use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};

use inkwell::context::Context;
use inkwell::OptimizationLevel;
use kyte::analyzer::{Analyzer, AnalyzerConfig, Severity};
use kyte::codegen::Codegen;
use kyte::lexer::Lexer;
use kyte::parser::Parser;

use super::cache;

/// Compile `ir_path` (.ll) to `obj_path` (.o).
///
/// Strategy 1 — external `llc`: avoids the STATUS_ACCESS_VIOLATION that
/// `LLVMTargetMachineEmitToMemoryBuffer` triggers on LLVM 21 + Windows.
/// Strategy 2 — inkwell `write_object_file`: fallback for platforms where
/// llc is not installed.
fn emit_object_file(
    ir_path: &str,
    obj_path: &str,
    codegen: &mut Codegen<'_>,
) -> Result<(), String> {
    // Search for llc in standard locations (Windows-first, then PATH).
    let candidates: &[&str] = &[
        r"C:\llvm\bin\llc.exe",
        r"C:\Program Files\LLVM\bin\llc.exe",
        "llc",
    ];

    for &llc in candidates {
        match std::process::Command::new(llc)
            .args([ir_path, "-filetype=obj", "-o", obj_path])
            .output()
        {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => {
                return Err(format!(
                    "llc exited with {}: {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            Err(_) => {} // not found or not executable — try next candidate
        }
    }

    // llc not available: fall back to inkwell.
    // Note: this path crashes on Windows LLVM 21 (STATUS_ACCESS_VIOLATION).
    catch_unwind(AssertUnwindSafe(|| {
        codegen.write_object_file(obj_path);
    }))
    .map_err(|panic| {
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "object file generation failed".to_string())
    })
}

pub(super) fn compile_source(
    source: &str,
    label: &str,
    opt_level: OptimizationLevel,
    debug_mode: bool,
    analyzer_config: &AnalyzerConfig,
    incremental: bool,
) {
    let start = std::time::Instant::now();

    // ── Incremental cache check ──────────────────────────────────────────────
    // Hash the merged source.  On a cache hit the entire compile pipeline is
    // skipped — we just copy the previously compiled .o and report timing.
    let source_hash = cache::hash_source(source);
    if incremental && label.ends_with(".ky") {
        let obj_path = cache::obj_path_for(label);
        if cache::try_cache_hit(source_hash, &obj_path) {
            let elapsed = start.elapsed();
            let ms = elapsed.as_millis();
            let time_str = if ms < 1 {
                "< 1ms".to_string()
            } else if ms < 1000 {
                format!("{}ms", ms)
            } else {
                format!("{:.2}s", elapsed.as_secs_f64())
            };
            println!("  cached — done in {} (no changes detected)", time_str);
            println!();
            let _ = std::io::stdout().flush();
            unsafe { crate::platform_exit(0) }
        }
    }

    // ── Lex ─────────────────────────────────────────────────────────────────
    let mut lex = Lexer::new(source);
    let tokens = lex.tokenize();

    if !lex.errors.is_empty() {
        for e in &lex.errors {
            eprintln!("  lex error: {}", e);
        }
    }

    // ── Parse ────────────────────────────────────────────────────────────────
    let ast_result = catch_unwind(AssertUnwindSafe(|| {
        let mut par = Parser::new(tokens, source);
        let program = par.parse();
        (program, par.errors)
    }));

    let (ast, parse_errors) = match ast_result {
        Ok((ast, errs)) => (ast, errs),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "Syntax error".to_string());
            println!("  parse failed: {}\n", msg);
            return;
        }
    };

    if !parse_errors.is_empty() {
        for e in &parse_errors {
            eprint!("{}", e);
        }
        println!("  build aborted: {} parse error(s)\n", parse_errors.len());
        return;
    }

    if !lex.errors.is_empty() {
        println!("  build aborted: {} lex error(s)\n", lex.errors.len());
        return;
    }

    // ── Semantic analysis ────────────────────────────────────────────────────
    let errors = Analyzer::analyze_with_config(&ast, source, analyzer_config.clone());
    let err_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    if err_count > 0 {
        for e in &errors {
            print!("{}", e);
        }
        println!("  build aborted: {} error(s)\n", err_count);
        return;
    }

    // ── LLVM codegen ─────────────────────────────────────────────────────────
    let context = Context::create();
    let obj_was_written = {
        let mut codegen = Codegen::new(&context);
        codegen.opt_level = opt_level;
        codegen.debug_mode = debug_mode;

        let codegen_result = catch_unwind(AssertUnwindSafe(|| {
            codegen.compile(&ast);
        }));

        if let Err(panic) = codegen_result {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "internal compiler error".to_string());
            eprintln!("  codegen failed: {}", msg);
            println!("  build aborted: codegen error\n");
            return;
        }

        let ir_path = if label.ends_with(".ky") {
            label.replace(".ky", ".ll")
        } else {
            "output.ll".to_string()
        };
        codegen.write_ir_file(&ir_path);

        // Cache the IR as well (for diagnostics / future incremental IR linking).
        if incremental {
            cache::save_ir_to_cache(source_hash, &ir_path);
        }

        let mut obj_written = false;
        if label.ends_with(".ky") {
            let obj_path = cache::obj_path_for(label);
            match emit_object_file(&ir_path, &obj_path, &mut codegen) {
                Ok(()) => {
                    if incremental {
                        cache::save_to_cache(source_hash, &obj_path);
                        cache::evict_stale(source_hash);
                    }
                    obj_written = true;
                }
                Err(msg) => {
                    eprintln!("  warning: object file write failed: {}", msg);
                    eprintln!("  (IR file was written to {})", ir_path);
                }
            }
        }

        std::mem::forget(codegen);
        obj_written
    };
    std::mem::forget(context);
    let _ = obj_was_written;

    let elapsed = start.elapsed();
    let ms = elapsed.as_millis();
    let time_str = if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.2}s", elapsed.as_secs_f64())
    };
    println!("  done in {}", time_str);
    println!();
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { crate::platform_exit(0) }
}

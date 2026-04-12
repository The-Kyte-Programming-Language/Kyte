use inkwell::context::Context;
use kyte::analyzer::{Analyzer, Severity};
use kyte::codegen::Codegen;
use kyte::lexer::Lexer;
use kyte::parser::Parser;
use std::env;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

fn safe_exit(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn TerminateProcess(handle: *mut std::ffi::c_void, exit_code: u32) -> i32;
        }
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    #[cfg(not(windows))]
    unsafe {
        extern "C" {
            fn _exit(code: i32) -> !;
        }
        _exit(code);
    }
    unreachable!()
}

fn print_banner() {
    println!(
        "\n  KYTE\n  Kyte Compiler v0.1.0  ·  LLVM 21\n"
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("lsp") => {
            if let Err(e) = kyte::lsp::run() {
                eprintln!("[kyte-lsp] fatal: {e}");
                std::process::exit(1);
            }
        }
        Some("test") => {
            print_banner();
            run_tests();
            safe_exit(0);
        }
        Some(path) => {
            print_banner();
            let source = load_source_with_imports(path).unwrap_or_else(|e| {
                eprintln!("  Error loading {}: {}", path, e);
                safe_exit(1);
            });
            compile_source(&source, path);
            safe_exit(0);
        }
        None => {
            print_banner();
            println!("  Usage:");
            println!("    kyte <file.ky>   Compile a Kyte source file");
            println!("    kyte lsp         Start the LSP server (stdio)");
            println!("    kyte test        Run built-in test suite");
            println!();
        }
    }
}

fn run_tests() {
    run_test(
        "basic",
        r#"
fn add(int a, int b) -> int { return a + b; }
@main(main) {
    int x = add(1, 2);
    print(x);
}
"#,
    );
}

fn run_test(label: &str, source: &str) {
    println!("  === {} ===", label);

    let parse_result = catch_unwind(AssertUnwindSafe(|| {
        let mut lex = Lexer::new(source);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        par.parse()
    }));

    let ast = match parse_result {
        Ok(ast) => ast,
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "Syntax error".to_string());
            println!("  parse failed: {}", msg);
            return;
        }
    };

    let errors = Analyzer::analyze(&ast, source);
    if errors.is_empty() {
        println!("  PASS\n");
        return;
    }

    let err_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .count();
    let warn_count = errors
        .iter()
        .filter(|e| e.severity == Severity::Warning)
        .count();

    for e in &errors {
        print!("{}", e);
    }
    println!("  {} error(s), {} warning(s)\n", err_count, warn_count);
}

fn parse_import_path(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with("import") {
        return None;
    }
    let rest = t["import".len()..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let end_quote = rest[1..].find('"')? + 1;
    let path = &rest[1..end_quote];
    let tail = rest[end_quote + 1..].trim();
    if tail != ";" {
        return None;
    }
    Some(path.to_string())
}

fn load_source_with_imports(entry: &str) -> Result<String, String> {
    fn visit(
        path: &Path,
        seen: &mut std::collections::HashSet<PathBuf>,
        out: &mut String,
    ) -> Result<(), String> {
        let canonical = fs::canonicalize(path).map_err(|e| format!("{} ({})", path.display(), e))?;
        if seen.contains(&canonical) {
            return Ok(());
        }
        seen.insert(canonical.clone());

        let text = fs::read_to_string(&canonical)
            .map_err(|e| format!("{} ({})", canonical.display(), e))?;
        let base_dir = canonical.parent().unwrap_or_else(|| Path::new("."));

        for line in text.lines() {
            if let Some(rel) = parse_import_path(line) {
                let dep = base_dir.join(rel);
                visit(&dep, seen, out)?;
            }
        }

        out.push_str(&format!("\n// ---- file: {} ----\n", canonical.display()));
        for line in text.lines() {
            if parse_import_path(line).is_none() {
                out.push_str(line);
                out.push('\n');
            }
        }
        Ok(())
    }

    let mut seen = std::collections::HashSet::new();
    let mut merged = String::new();
    visit(Path::new(entry), &mut seen, &mut merged)?;
    Ok(merged)
}

fn compile_source(source: &str, label: &str) {
    let start = std::time::Instant::now();

    let ast = match catch_unwind(AssertUnwindSafe(|| {
        let mut lex = Lexer::new(source);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        par.parse()
    })) {
        Ok(ast) => ast,
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

    let errors = Analyzer::analyze(&ast, source);
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

    let context = Context::create();
    let mut codegen = Codegen::new(&context);
    codegen.compile(&ast);

    let ir_path = if label.ends_with(".ky") {
        label.replace(".ky", ".ll")
    } else {
        "output.ll".to_string()
    };
    codegen.write_ir_file(&ir_path);

    if label.ends_with(".ky") {
        codegen.write_object_file(&label.replace(".ky", ".o"));
    }

    let elapsed = start.elapsed();
    let ms = elapsed.as_millis();
    let time_str = if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.2}s", elapsed.as_secs_f64())
    };
    println!("  done in {}", time_str);
    println!();

    safe_exit(0);
}
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};

use inkwell::context::Context;
use inkwell::OptimizationLevel;
use kyte::analyzer::{Analyzer, AnalyzerConfig, Severity};
use kyte::codegen::Codegen;
use kyte::lexer::Lexer;
use kyte::parser::Parser;

pub(super) fn compile_source(
    source: &str,
    label: &str,
    opt_level: OptimizationLevel,
    debug_mode: bool,
    analyzer_config: &AnalyzerConfig,
) {
    let start = std::time::Instant::now();

    let mut lex = Lexer::new(source);
    let tokens = lex.tokenize();

    if !lex.errors.is_empty() {
        for e in &lex.errors {
            eprintln!("  lex error: {}", e);
        }
    }

    let ast_result = catch_unwind(AssertUnwindSafe(|| {
        let mut par = Parser::new(tokens);
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
            eprintln!("  parse error: {}", e);
        }
        println!("  build aborted: {} parse error(s)\n", parse_errors.len());
        return;
    }

    if !lex.errors.is_empty() {
        println!("  build aborted: {} lex error(s)\n", lex.errors.len());
        return;
    }

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

    let context = Context::create();
    let _ir_path = {
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

        if label.ends_with(".ky") {
            let obj_path = label.replace(".ky", ".o");
            let obj_result = catch_unwind(AssertUnwindSafe(|| {
                codegen.write_object_file(&obj_path);
            }));
            if let Err(panic) = obj_result {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "object file generation failed".to_string());
                eprintln!("  warning: object file write failed: {}", msg);
                eprintln!("  (IR file was written to {})", ir_path);
            }
        }
        // codegen drop 전에 LLVM context 해제 충돌 방지
        std::mem::forget(codegen);
        ir_path
    };
    std::mem::forget(context);

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
    // LLVM 전역 상태(atexit 핸들러) drop 전에 프로세스 종료
    unsafe { crate::platform_exit(0) }
}

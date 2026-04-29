use inkwell::OptimizationLevel;
use kyte::analyzer::AnalyzerConfig;
use std::env;
use std::io::Write;

#[path = "main/compile.rs"]
mod compile;
#[path = "main/imports.rs"]
mod imports;
#[path = "main/test_runner.rs"]
mod test_runner;

use compile::compile_source;
use imports::load_source_with_imports;
use test_runner::run_tests;

const C_RED: &str = "\x1b[31m";
const C_GREEN: &str = "\x1b[32m";
const C_YELLOW: &str = "\x1b[33m";
const C_CYAN: &str = "\x1b[36m";
const C_DIM: &str = "\x1b[2m";
const C_RESET: &str = "\x1b[0m";

#[cfg(windows)]
unsafe fn platform_exit(code: i32) -> ! {
    extern "system" {
        fn ExitProcess(exit_code: u32) -> !;
    }
    ExitProcess(code as u32);
}

#[cfg(not(windows))]
unsafe fn platform_exit(code: i32) -> ! {
    extern "C" {
        fn _exit(code: i32) -> !;
    }
    _exit(code);
}

fn safe_exit(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { platform_exit(code) }
}

fn print_banner() {
    println!("\n  KYTE\n  Kyte Compiler v0.1.0  ·  LLVM 21\n");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // 플래그 파싱 (A03, A05)
    let release = args.iter().any(|a| a == "--release");
    let wall = args.iter().any(|a| a == "--Wall");
    let werror = args.iter().any(|a| a == "--Werror");
    let no_unused = args.iter().any(|a| a == "--no-unused");

    let analyzer_config = AnalyzerConfig {
        wall,
        werror,
        no_unused,
    };
    let opt_level = if release {
        OptimizationLevel::Aggressive
    } else {
        OptimizationLevel::None
    };
    let debug_mode = !release;

    // 서브커맨드 추출 (플래그가 아닌 첫 번째 인자)
    let subcommand = args.iter().skip(1).find(|a| !a.starts_with("--"));

    match subcommand.map(|s| s.as_str()) {
        Some("lsp") => {
            if let Err(e) = kyte::lsp::run() {
                eprintln!("[kyte-lsp] fatal: {e}");
                std::process::exit(1);
            }
        }
        Some("test") => {
            print_banner();
            let ok = run_tests();
            safe_exit(if ok { 0 } else { 1 });
        }
        Some(path) => {
            print_banner();
            let source = load_source_with_imports(path).unwrap_or_else(|e| {
                eprintln!("  Error loading {}: {}", path, e);
                safe_exit(1);
            });
            compile_source(&source, path, opt_level, debug_mode, &analyzer_config);
            safe_exit(0);
        }
        None => {
            print_banner();
            println!("  Usage:");
            println!("    kyte <file.ky>   Compile a Kyte source file");
            println!("    kyte lsp         Start the LSP server (stdio)");
            println!("    kyte test        Run built-in test suite");
            println!();
            println!("  Flags:");
            println!("    --release        Optimize (O3) and disable overflow traps");
            println!("    --Wall           Enable all warnings");
            println!("    --Werror         Treat warnings as errors");
            println!("    --no-unused      Suppress unused variable warnings");
            println!();
        }
    }
}

use inkwell::OptimizationLevel;
use kyte::analyzer::AnalyzerConfig;
use std::env;
use std::io::Write;

#[path = "main/cache.rs"]
mod cache;
#[path = "main/compile.rs"]
mod compile;
#[path = "main/imports.rs"]
mod imports;
#[path = "main/test_runner.rs"]
mod test_runner;

use compile::compile_source;
use imports::{count_source_files, load_source_parallel, load_source_with_imports};
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

    // ── Flag parsing ─────────────────────────────────────────────────────────
    let release = args.iter().any(|a| a == "--release");
    let wall = args.iter().any(|a| a == "--Wall");
    let werror = args.iter().any(|a| a == "--Werror");
    let no_unused = args.iter().any(|a| a == "--no-unused");
    // --no-cache disables incremental compilation; incremental is on by default.
    let incremental = !args.iter().any(|a| a == "--no-cache");

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

    // ── Subcommand dispatch ───────────────────────────────────────────────────
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

            // Count source files for the build header.
            let file_count = count_source_files(path);
            if file_count > 1 {
                println!(
                    "  {} source file{} (parallel I/O)",
                    file_count,
                    if file_count == 1 { "" } else { "s" }
                );
            }

            // Use parallel loader when there are multiple files; sequential
            // otherwise (avoids thread-spawn overhead on single-file builds).
            let source = if file_count > 1 {
                load_source_parallel(path)
            } else {
                load_source_with_imports(path)
            }
            .unwrap_or_else(|e| {
                eprintln!("  Error loading {}: {}", path, e);
                safe_exit(1);
            });

            compile_source(
                &source,
                path,
                opt_level,
                debug_mode,
                &analyzer_config,
                incremental,
            );
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
            println!("    --no-cache       Disable incremental build cache");
            println!();
        }
    }
}

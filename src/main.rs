mod lexer;
mod ast;
mod parser;
mod analyzer;
mod codegen;

use lexer::Lexer;
use parser::Parser;
use analyzer::{Analyzer, Severity};

fn main() {
    println!(r#"
  ██╗  ██╗██╗   ██╗████████╗███████╗
  ██║ ██╔╝╚██╗ ██╔╝╚══██╔══╝██╔════╝
  █████╔╝  ╚████╔╝    ██║   █████╗  
  ██╔═██╗   ╚██╔╝     ██║   ██╔══╝  
  ██║  ██╗   ██║      ██║   ███████╗
  ╚═╝  ╚═╝   ╚═╝      ╚═╝   ╚══════╝
  
  Kyte Compiler v0.1.0
  [ LLVM V21 ]
  "#);
  

let source = r#"
fn add(int a, int b) -> int {
    return a + b;
}

fn add(int x) -> int {
    return x;
}

@main(main)
    int x = 10;
    int x = 20;
    int y = "hello";
    z = 5;
    x += "bad";
    int r = add(1, 2);
    int q = add(1);
    ghost(42);
    if x {
        int a = 1;
    }
"#;

    let mut lex = Lexer::new(source);
    let tokens  = lex.tokenize();
    let mut par = Parser::new(tokens);
    let ast     = par.parse();

    let errors = Analyzer::analyze(&ast, source);
    if !errors.is_empty() {
        let err_count  = errors.iter().filter(|e| e.severity == Severity::Error).count();
        let warn_count = errors.iter().filter(|e| e.severity == Severity::Warning).count();

        println!();
        for e in &errors {
            print!("{}", e);
        }

        println!("\n  \x1b[1;90m─────────────────────────────────────────\x1b[0m");
        if err_count > 0 {
            print!("  \x1b[1;31m{} error(s)\x1b[0m", err_count);
        }
        if warn_count > 0 {
            if err_count > 0 { print!(", "); }
            print!("\x1b[1;33m{} warning(s)\x1b[0m", warn_count);
        }
        println!("\n");
    } else {
        println!("{:#?}", ast);
    }
}
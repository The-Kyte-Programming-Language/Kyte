use kyte::lexer::Lexer;
use kyte::parser::Parser;
use kyte::analyzer::{Analyzer, Severity};
use kyte::codegen::Codegen;
use inkwell::context::Context;
use std::env;
use std::fs;

/// LLVM atexit 핸들러가 전역 상태 정리 중 접근 위반을 일으킬 수 있어
/// TerminateProcess(Win) / _exit(Unix)로 우회한다.
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
        extern "C" { fn _exit(code: i32) -> !; }
        _exit(code);
    }
    unreachable!()
}

fn print_banner() {
    println!(r#"
  ██╗  ██╗██╗   ██╗████████╗███████╗
  ██║ ██╔╝╚██╗ ██╔╝╚══██╔══╝██╔════╝
  █████╔╝  ╚████╔╝    ██║   █████╗  
  ██╔═██╗   ╚██╔╝     ██║   ██╔══╝  
  ██║  ██╗   ██║      ██║   ███████╗
  ╚═╝  ╚═╝   ╚═╝      ╚═╝   ╚══════╝
  Kyte Compiler v0.1.0  ·  LLVM 21
"#);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        // ── LSP 서버 ──
        Some("lsp") => {
            if let Err(e) = kyte::lsp::run() {
                eprintln!("[kyte-lsp] fatal: {e}");
                std::process::exit(1);
            }
        }

        // ── 내장 테스트 ──
        Some("test") => {
            print_banner();
            run_tests();
            run_feature_tests();
            safe_exit(0);
        }

        // ── .ky 파일 컴파일 ──
        Some(path) => {
            print_banner();
            let source = fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("  Error reading {}: {}", path, e);
                safe_exit(1);
            });
            compile_source(&source, path);
            safe_exit(0);
        }

        // ── 사용법 ──
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

// ─────────────────────────────────────────
//  내장 테스트 모음
// ─────────────────────────────────────────

fn run_tests() {
    run_test("1. Error Detection", r#"
fn add(int a, int b) -> int { return a + b; }
fn add(int x) -> int { return x; }
@main(main) {
    int x = 10;
    int x = 20;
    int y = "hello";
    z = 5;
    x += "bad";
    int r = add(1, 2);
    int q = add(1);
    ghost(42);
    if x { int a = 1; }
}
"#);

    run_test("2. Clean Code (expect 0 errors)", r#"
fn square(int n) -> int { return n * n; }
fn greet(string name) -> string { return "Hello, " + name; }
fn add(int a, int b) -> int { return a + b; }
@app(main) {
    int x = 10;
    int y = 20;
    int sum = add(x, y);
    bool flag = x > y;
    if flag { int z = 100; }
    for i in 0..5 { int temp = i * 2; }
}
"#);

    run_test("3. Anchor Scope Isolation", r#"
@outer(main) {
    int a = 10;
    @inner(thread) {
        int b = 20;
    }
    a = b;
}
"#);

    run_test("4. Type System", r#"
@main(main) {
    int a = 10;
    float b = 3.14;
    string c = "hello";
    bool d = true;
    int e = a + b;
    bool f = a && d;
    string g = c - a;
    int h = -c;
    bool i = !a;
}
"#);

    run_test("5. Function Signature Validation", r#"
fn calc(int x, int y, bool flag) -> int { return x + y; }
@main(main) {
    int r1 = calc(1, 2, true);
    int r2 = calc(1, 2);
    int r3 = calc(1, 2, 3);
    int r4 = calc("a", "b", true);
}
"#);

    run_test("6. Compound Assign + Vault", r#"
@main(main) {
    int x = 10;
    x += 5; x -= 2; x *= 3; x /= 2; x %= 7;
    x += 3.14;
    Vault int buf = 1024;
    loop { Vault int temp = 256; }
    loop { Vault int safe = 128; free(safe); }
}
"#);

    run_test("7. Return Type + Control Flow", r#"
fn bad_return(int x) -> int { return "oops"; }
fn no_return(int x) -> float { return; }
fn ok(int x) -> bool { return x > 0; }
@main(main) {
    int a = ok(5);
}
"#);

    run_test("8. Nested Anchor Scope", r#"
@root(main) {
    int shared = 42;
    @child1(thread) {
        int local1 = shared + 1;
        @grandchild(thread) {
            int deep = local1 + shared;
        }
    }
    @child2(thread) {
        int local2 = local1;
        int ok2 = shared;
    }
}
"#);

    println!("  \x1b[1;36m━━━ 9. Codegen (LLVM IR) ━━━\x1b[0m");
    compile_source(r#"
fn add(int a, int b) -> int { return a + b; }
fn square(int n) -> int { return n * n; }
@app(main) {
    int x = 10;
    int y = 20;
    int sum = add(x, y);
    int sq = square(sum);
    print(sum);
    print(sq);
    bool flag = x > y;
    if flag { print(1); } else { print(0); }
    for i in 0..5 { print(i); }
}
"#, "<inline>");
}

// ─────────────────────────────────────────
//  추가 기능 통합 테스트
// ─────────────────────────────────────────

fn run_feature_tests() {
    // print 테스트
    run_test("F1. print statement", r#"
@app(main) {
    print(42);
    print("hello");
    print(3.14);
    print(true);
}
"#);

    // string concat 테스트
    run_test("F2. string concat", r#"
@app(main) {
    string a = "Hello";
    string b = " World";
    string c = a + b;
    print(c);
}
"#);

    // Vault + free 테스트
    run_test("F3. Vault heap allocation", r#"
@app(main) {
    Vault int x = 42;
    print(x);
    free(x);
}
"#);

    // Kill → anchor recovery 테스트
    run_test("F4. Kill recovery", r#"
@app(main) {
    int x = 10;
    @handler() {
        Kill "error in handler";
    }
    print(x);
}
"#);

    // yield → data transfer 테스트
    run_test("F5. yield in anchor", r#"
@app(main) {
    int a = 100;
    @producer() {
        yield a;
    }
    print(a);
}
"#);

    // 복합 테스트
    run_test("F6. Combined features", r#"
fn greet(string name) -> string { return "Hello, " + name; }
@app(main) {
    string msg = greet("Kyte");
    print(msg);
    Vault int buf = 1024;
    print(buf);
    free(buf);
    @safe() {
        Kill "recovered!";
    }
    print("done");
}
"#);

    // while 루프 테스트
    run_test("F7. while loop", r#"
@app(main) {
    int i = 0;
    while i < 5 {
        print(i);
        i += 1;
    }
}
"#);

    // as 타입 캐스팅 테스트
    run_test("F8. type casting", r#"
@app(main) {
    int x = 42;
    float y = x as float;
    print(y);
    float pi = 3.14;
    int rounded = pi as int;
    print(rounded);
}
"#);

    // string + int 자동 변환 테스트
    run_test("F9. string + int concat", r#"
@app(main) {
    string msg = "score: " + 100;
    print(msg);
    string msg2 = "pi = " + 3.14;
    print(msg2);
}
"#);

    // string 비교 테스트
    run_test("F10. string comparison", r#"
@app(main) {
    string a = "hello";
    string b = "hello";
    string c = "world";
    bool same = a == b;
    bool diff = a != c;
    print(same);
    print(diff);
}
"#);
}

// ─────────────────────────────────────────
//  헬퍼
// ─────────────────────────────────────────

fn run_test(label: &str, source: &str) {
    println!("  \x1b[1;36m━━━ {} ━━━\x1b[0m", label);

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

        println!("  \x1b[1;90m─────────────────────────────────────────\x1b[0m");
        if err_count > 0 {
            print!("  \x1b[1;31m{} error(s)\x1b[0m", err_count);
        }
        if warn_count > 0 {
            if err_count > 0 { print!(", "); }
            print!("\x1b[1;33m{} warning(s)\x1b[0m", warn_count);
        }
        println!("\n");
    } else {
        println!("  \x1b[1;32m✓ PASS — no errors\x1b[0m\n");
    }
}

fn compile_source(source: &str, label: &str) {
    let start = std::time::Instant::now();

    let mut lex = Lexer::new(source);
    let tokens  = lex.tokenize();
    let mut par = Parser::new(tokens);
    let ast     = par.parse();

    let errors = Analyzer::analyze(&ast, source);
    let err_count = errors.iter().filter(|e| e.severity == Severity::Error).count();

    if !errors.is_empty() {
        println!();
        for e in &errors {
            print!("{}", e);
        }
        if err_count > 0 {
            println!("  \x1b[1;31m✗ {} error(s) — compilation aborted\x1b[0m\n", err_count);
            return;
        }
    }

    let context = Context::create();
    let mut codegen = Codegen::new(&context);
    codegen.compile(&ast);

    println!("  \x1b[1;32m✓ LLVM IR generated\x1b[0m");

    // IR을 파일로 출력 (LLVM 문자열 해제 크래시 우회)
    let ir_tmp = if label.ends_with(".ky") {
        label.replace(".ky", ".ll")
    } else {
        String::from("output.ll")
    };
    codegen.write_ir_file(&ir_tmp);

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
    println!("  \x1b[1;32m✓ done\x1b[0m  \x1b[1;90min {}\x1b[0m", time_str);
    println!();

    // LLVM Context/Module 정리 시 접근 위반 회피 — 프로세스 즉시 종료
    safe_exit(0);
}
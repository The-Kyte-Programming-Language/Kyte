use std::collections::HashMap;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use kyte::analyzer::{Analyzer, Severity};
use kyte::ast::{Program, TopLevel};
use kyte::lexer::Lexer;
use kyte::parser::Parser;

use super::imports::load_source_with_imports;
use super::{C_CYAN, C_DIM, C_GREEN, C_RED, C_RESET, C_YELLOW};

#[derive(Clone, Debug)]
enum DecoratorExpectation {
    Pass(Option<String>),
    Fail(Option<String>),
    Skip,
}

#[derive(Clone, Debug)]
struct DecoratorTestCase {
    name: String,
    expectation: DecoratorExpectation,
    valid_signature: bool,
}

#[derive(Clone, Debug)]
enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug)]
struct TestResultItem {
    name: String,
    status: TestStatus,
    detail: String,
}

fn parse_expected_value(raw: &str) -> Result<ExpectedValue, String> {
    let v = raw.trim();
    if v.eq_ignore_ascii_case("true") {
        return Ok(ExpectedValue::Bool(true));
    }
    if v.eq_ignore_ascii_case("false") {
        return Ok(ExpectedValue::Bool(false));
    }
    // 쌍따옴표로 감싸여 있으면 string
    if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
        return Ok(ExpectedValue::Str(v[1..v.len() - 1].to_string()));
    }
    // float 시도 (소수점 포함)
    if v.contains('.') {
        if let Ok(f) = v.parse::<f64>() {
            return Ok(ExpectedValue::Float(f));
        }
    }
    // int 시도
    if let Ok(n) = v.parse::<i64>() {
        return Ok(ExpectedValue::Int(n));
    }
    Err(format!(
        "unsupported expected value '{}': use int, float, bool, or \"string\"",
        raw
    ))
}

#[derive(Clone, Debug)]
enum ExpectedValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

impl std::fmt::Display for ExpectedValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpectedValue::Int(n) => write!(f, "{}", n),
            ExpectedValue::Float(v) => write!(f, "{}", v),
            ExpectedValue::Bool(b) => write!(f, "{}", b),
            ExpectedValue::Str(s) => write!(f, "\"{}\"", s),
        }
    }
}

#[derive(Clone, Debug)]
enum ActualValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

impl std::fmt::Display for ActualValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActualValue::Int(n) => write!(f, "{}", n),
            ActualValue::Float(v) => write!(f, "{}", v),
            ActualValue::Bool(b) => write!(f, "{}", b),
            ActualValue::Str(s) => write!(f, "\"{}\"", s),
        }
    }
}

fn values_equal(expected: &ExpectedValue, actual: &ActualValue) -> bool {
    match (expected, actual) {
        (ExpectedValue::Int(e), ActualValue::Int(a)) => *e == *a,
        (ExpectedValue::Float(e), ActualValue::Float(a)) => (*e - *a).abs() < 1e-9,
        (ExpectedValue::Bool(e), ActualValue::Bool(a)) => *e == *a,
        (ExpectedValue::Str(e), ActualValue::Str(a)) => e == a,
        // int ↔ bool 호환
        (ExpectedValue::Bool(e), ActualValue::Int(a)) => (*e as i64) == *a,
        (ExpectedValue::Int(e), ActualValue::Bool(a)) => *e == (*a as i64),
        _ => false,
    }
}

fn collect_expected_value_results(
    ast: &Program,
    cases: &[DecoratorTestCase],
) -> HashMap<String, Result<ActualValue, String>> {
    let mut targets: HashMap<String, ()> = HashMap::new();
    for c in cases {
        if c.valid_signature {
            if let DecoratorExpectation::Pass(Some(_)) = &c.expectation {
                targets.insert(c.name.clone(), ());
            }
        }
    }
    if targets.is_empty() {
        return HashMap::new();
    }

    // AST에서 대상 함수 본문을 찾아 간단히 인터프리트
    let mut out: HashMap<String, Result<ActualValue, String>> = HashMap::new();
    let all_fns = collect_all_fns(ast);
    for name in targets.keys() {
        if let Some(fi) = all_fns.get(name) {
            let env = HashMap::new();
            out.insert(
                name.clone(),
                eval_fn_body_env(&fi.body, &fi.ret_ty, &env, &all_fns),
            );
        } else {
            out.insert(name.clone(), Err(format!("function '{}' not found", name)));
        }
    }
    out
}

#[derive(Clone)]
struct FnInfo {
    params: Vec<(String, kyte::ast::Ty)>,
    ret_ty: Option<kyte::ast::Ty>,
    body: Vec<(kyte::ast::Stmt, kyte::ast::Span)>,
}

fn collect_all_fns(ast: &Program) -> HashMap<String, FnInfo> {
    let mut fns = HashMap::new();
    for (item, _) in &ast.items {
        if let TopLevel::Function {
            name,
            params,
            body,
            return_ty,
            ..
        } = item
        {
            fns.insert(
                name.clone(),
                FnInfo {
                    params: params
                        .iter()
                        .map(|p| (p.name.clone(), p.ty.clone()))
                        .collect(),
                    ret_ty: return_ty.clone(),
                    body: body.clone(),
                },
            );
        }
    }
    fns
}

type Env = HashMap<String, ActualValue>;

fn eval_fn_body_env(
    body: &[(kyte::ast::Stmt, kyte::ast::Span)],
    ret_ty: &Option<kyte::ast::Ty>,
    env: &Env,
    fns: &HashMap<String, FnInfo>,
) -> Result<ActualValue, String> {
    let mut local_env = env.clone();
    for (stmt, _) in body {
        match eval_stmt(stmt, ret_ty, &mut local_env, fns)? {
            StmtResult::Continue => {}
            StmtResult::Return(val) => return Ok(val),
        }
    }
    Err("test function has no return statement".to_string())
}

enum StmtResult {
    Continue,
    Return(ActualValue),
}

fn eval_stmt(
    stmt: &kyte::ast::Stmt,
    ret_ty: &Option<kyte::ast::Ty>,
    env: &mut Env,
    fns: &HashMap<String, FnInfo>,
) -> Result<StmtResult, String> {
    use kyte::ast::Stmt;
    match stmt {
        Stmt::VarDecl { name, value, .. } | Stmt::ConstDecl { name, value, .. } => {
            let val = eval_expr_env(value, ret_ty, env, fns)?;
            env.insert(name.clone(), val);
            Ok(StmtResult::Continue)
        }
        Stmt::Assign { name, value } => {
            let val = eval_expr_env(value, ret_ty, env, fns)?;
            env.insert(name.clone(), val);
            Ok(StmtResult::Continue)
        }
        Stmt::Return(Some(expr)) => {
            let val = eval_expr_env(expr, ret_ty, env, fns)?;
            Ok(StmtResult::Return(val))
        }
        Stmt::Return(None) => Err("test function returns void".to_string()),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let cond_val = eval_expr_env(cond, ret_ty, env, fns)?;
            let is_true = match &cond_val {
                ActualValue::Bool(b) => *b,
                ActualValue::Int(n) => *n != 0,
                _ => return Err("if condition must be bool or int".to_string()),
            };
            let branch = if is_true {
                then_body
            } else {
                match else_body {
                    Some(eb) => eb,
                    None => return Ok(StmtResult::Continue),
                }
            };
            for (s, _) in branch {
                match eval_stmt(s, ret_ty, env, fns)? {
                    StmtResult::Continue => {}
                    StmtResult::Return(v) => return Ok(StmtResult::Return(v)),
                }
            }
            Ok(StmtResult::Continue)
        }
        Stmt::ExprStmt(_) | Stmt::Print(_) => Ok(StmtResult::Continue), // side-effect only
        _ => Err(format!(
            "unsupported statement in test evaluation: {:?}",
            std::mem::discriminant(stmt)
        )),
    }
}

#[allow(clippy::only_used_in_recursion)]
fn eval_expr_env(
    expr: &kyte::ast::Expr,
    hint_ty: &Option<kyte::ast::Ty>,
    env: &Env,
    fns: &HashMap<String, FnInfo>,
) -> Result<ActualValue, String> {
    use kyte::ast::{Expr, UnaryOpKind};
    match expr {
        Expr::IntLit(n) => Ok(ActualValue::Int(*n)),
        Expr::FloatLit(f) => Ok(ActualValue::Float(*f)),
        Expr::Bool(b) => Ok(ActualValue::Bool(*b)),
        Expr::StringLit(s) => Ok(ActualValue::Str(s.clone())),
        Expr::Ident(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined variable '{}' in test evaluation", name)),
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr: inner,
        } => match eval_expr_env(inner, hint_ty, env, fns)? {
            ActualValue::Int(n) => Ok(ActualValue::Int(-n)),
            ActualValue::Float(f) => Ok(ActualValue::Float(-f)),
            other => Err(format!("cannot negate {}", other)),
        },
        Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr: inner,
        } => match eval_expr_env(inner, hint_ty, env, fns)? {
            ActualValue::Bool(b) => Ok(ActualValue::Bool(!b)),
            ActualValue::Int(n) => Ok(ActualValue::Bool(n == 0)),
            other => Err(format!("cannot apply not to {}", other)),
        },
        Expr::BinOp { left, op, right } => {
            let l = eval_expr_env(left, hint_ty, env, fns)?;
            let r = eval_expr_env(right, hint_ty, env, fns)?;
            eval_binop(&l, op, &r)
        }
        Expr::Call { name, args } => {
            if let Some(fi) = fns.get(name) {
                if fi.params.len() != args.len() {
                    return Err(format!(
                        "function '{}' expects {} args, got {}",
                        name,
                        fi.params.len(),
                        args.len()
                    ));
                }
                let mut call_env = HashMap::new();
                for (arg_expr, (param_name, _param_ty)) in args.iter().zip(fi.params.iter()) {
                    let val = eval_expr_env(arg_expr, hint_ty, env, fns)?;
                    call_env.insert(param_name.clone(), val);
                }
                eval_fn_body_env(&fi.body, &fi.ret_ty, &call_env, fns)
            } else {
                Err(format!("function '{}' not found for test evaluation", name))
            }
        }
        Expr::Cast { expr: inner, ty } => {
            let val = eval_expr_env(inner, &Some(ty.clone()), env, fns)?;
            eval_cast(val, ty)
        }
        _ => Err("expression too complex for test evaluation (use simpler return)".to_string()),
    }
}

fn eval_binop(
    l: &ActualValue,
    op: &kyte::ast::BinOpKind,
    r: &ActualValue,
) -> Result<ActualValue, String> {
    use kyte::ast::BinOpKind::*;
    match (l, r) {
        (ActualValue::Int(a), ActualValue::Int(b)) => match op {
            Add => Ok(ActualValue::Int(a + b)),
            Sub => Ok(ActualValue::Int(a - b)),
            Mul => Ok(ActualValue::Int(a * b)),
            Div => {
                if *b == 0 {
                    return Err("division by zero".to_string());
                }
                Ok(ActualValue::Int(a / b))
            }
            Mod => {
                if *b == 0 {
                    return Err("modulo by zero".to_string());
                }
                Ok(ActualValue::Int(a % b))
            }
            Eq => Ok(ActualValue::Bool(a == b)),
            Neq => Ok(ActualValue::Bool(a != b)),
            Lt => Ok(ActualValue::Bool(a < b)),
            Gt => Ok(ActualValue::Bool(a > b)),
            Le => Ok(ActualValue::Bool(a <= b)),
            Ge => Ok(ActualValue::Bool(a >= b)),
            _ => Err(format!("unsupported int op {:?}", op)),
        },
        (ActualValue::Float(a), ActualValue::Float(b)) => match op {
            Add => Ok(ActualValue::Float(a + b)),
            Sub => Ok(ActualValue::Float(a - b)),
            Mul => Ok(ActualValue::Float(a * b)),
            Div => Ok(ActualValue::Float(a / b)),
            Eq => Ok(ActualValue::Bool((a - b).abs() < 1e-9)),
            Neq => Ok(ActualValue::Bool((a - b).abs() >= 1e-9)),
            Lt => Ok(ActualValue::Bool(a < b)),
            Gt => Ok(ActualValue::Bool(a > b)),
            Le => Ok(ActualValue::Bool(a <= b)),
            Ge => Ok(ActualValue::Bool(a >= b)),
            _ => Err(format!("unsupported float op {:?}", op)),
        },
        (ActualValue::Bool(a), ActualValue::Bool(b)) => match op {
            And => Ok(ActualValue::Bool(*a && *b)),
            Or => Ok(ActualValue::Bool(*a || *b)),
            Eq => Ok(ActualValue::Bool(a == b)),
            Neq => Ok(ActualValue::Bool(a != b)),
            _ => Err(format!("unsupported bool op {:?}", op)),
        },
        (ActualValue::Str(a), ActualValue::Str(b)) => match op {
            Add => Ok(ActualValue::Str(format!("{}{}", a, b))),
            Eq => Ok(ActualValue::Bool(a == b)),
            Neq => Ok(ActualValue::Bool(a != b)),
            _ => Err(format!("unsupported string op {:?}", op)),
        },
        // int ↔ float 자동 승격
        (ActualValue::Int(a), ActualValue::Float(b)) => {
            eval_binop(&ActualValue::Float(*a as f64), op, &ActualValue::Float(*b))
        }
        (ActualValue::Float(a), ActualValue::Int(b)) => {
            eval_binop(&ActualValue::Float(*a), op, &ActualValue::Float(*b as f64))
        }
        _ => Err("type mismatch in test evaluation".to_string()),
    }
}

fn eval_cast(val: ActualValue, ty: &kyte::ast::Ty) -> Result<ActualValue, String> {
    use kyte::ast::Ty;
    match ty {
        Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 => {
            match val {
                ActualValue::Int(n) => Ok(ActualValue::Int(n)),
                ActualValue::Float(f) => Ok(ActualValue::Int(f as i64)),
                ActualValue::Bool(b) => Ok(ActualValue::Int(b as i64)),
                _ => Err("cannot cast to int".to_string()),
            }
        }
        Ty::Float => match val {
            ActualValue::Int(n) => Ok(ActualValue::Float(n as f64)),
            ActualValue::Float(f) => Ok(ActualValue::Float(f)),
            _ => Err("cannot cast to float".to_string()),
        },
        Ty::Bool => match val {
            ActualValue::Int(n) => Ok(ActualValue::Bool(n != 0)),
            ActualValue::Bool(b) => Ok(ActualValue::Bool(b)),
            _ => Err("cannot cast to bool".to_string()),
        },
        _ => Err(format!("unsupported cast target type {:?}", ty)),
    }
}

fn decorator_expectation(decorators: &[String]) -> Option<DecoratorExpectation> {
    for d in decorators {
        if d == "test" || d == "test(success)" {
            return Some(DecoratorExpectation::Pass(None));
        }
        if let Some(inner) = d.strip_prefix("test(").and_then(|s| s.strip_suffix(')')) {
            let mut parts = inner.splitn(2, ',');
            let kind = parts.next().unwrap_or("").trim();
            let arg = parts
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            if kind == "skip" {
                return Some(DecoratorExpectation::Skip);
            }
            if kind == "success" {
                return Some(DecoratorExpectation::Pass(arg));
            }
            if kind == "fail" {
                return Some(DecoratorExpectation::Fail(None));
            }
            if let Some(code) = kind.strip_prefix("fail_") {
                if !code.is_empty() {
                    return Some(DecoratorExpectation::Fail(Some(code.to_string())));
                }
                return Some(DecoratorExpectation::Fail(None));
            }
        }
    }
    None
}

fn collect_decorator_tests(ast: &Program) -> Vec<DecoratorTestCase> {
    let mut out = Vec::new();
    for (item, _) in &ast.items {
        if let TopLevel::Function {
            name,
            params,
            decorators,
            ..
        } = item
        {
            if let Some(expectation) = decorator_expectation(decorators) {
                out.push(DecoratorTestCase {
                    name: name.clone(),
                    expectation,
                    valid_signature: params.is_empty(),
                });
            }
        }
    }
    out
}

fn collect_ky_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("ky") {
                out.push(p);
            }
        }
    }

    let mut files = Vec::new();
    if root.exists() {
        walk(root, &mut files);
    }
    files.sort();
    files
}

pub(super) fn run_tests() -> bool {
    let cases_root = Path::new("test").join("cases");
    let files = collect_ky_files(&cases_root);
    if files.is_empty() {
        println!("  no test files found under {}", cases_root.display());
        println!("  expected decorator tests like: #test, #test(success,42), #test(fail), #test(fail_E004), #test(skip)\n");
        println!("  rust-style attributes are also supported: #[test], #[test(success,42)], #[test(fail)], #[test(skip)]\n");
        return false;
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut suite_passed = 0usize;
    let mut suite_failed = 0usize;
    let mut suite_skipped = 0usize;

    for file in files {
        let label = file.display().to_string();
        let source = match load_source_with_imports(file.to_string_lossy().as_ref()) {
            Ok(s) => s,
            Err(e) => {
                println!("  {C_RED}FAIL{C_RESET} {label}");
                println!("    {C_RED}✗{C_RESET} import load error: {}\n", e);
                failed += 1;
                continue;
            }
        };
        let (p, f, s, sf, ss) = run_test(&label, &source);
        passed += p;
        failed += f;
        skipped += s;
        if sf {
            suite_failed += 1;
        } else if ss {
            suite_skipped += 1;
        } else {
            suite_passed += 1;
        }
    }

    let total = passed + failed + skipped;
    let total_suites = suite_passed + suite_failed + suite_skipped;
    println!(
        "  Test Suites: {C_RED}{}{C_RESET} failed, {C_YELLOW}{}{C_RESET} skipped, {C_GREEN}{}{C_RESET} passed, {} total",
        suite_failed, suite_skipped, suite_passed, total_suites
    );
    println!(
        "  Tests:       {C_RED}{}{C_RESET} failed, {C_YELLOW}{}{C_RESET} skipped, {C_GREEN}{}{C_RESET} passed, {} total\n",
        failed, skipped, passed, total
    );
    failed == 0
}

fn run_test(label: &str, source: &str) -> (usize, usize, usize, bool, bool) {
    let mut lex = Lexer::new(source);
    let tokens = lex.tokenize();

    let parse_result = catch_unwind(AssertUnwindSafe(|| {
        let mut par = Parser::new(tokens);
        let program = par.parse();
        (program, par.errors)
    }));

    let (ast_opt, parse_errors) = match parse_result {
        Ok((ast, errs)) => (Some(ast), errs),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "Syntax error".to_string());
            println!("  {C_RED}FAIL{C_RESET} {label}");
            println!("    {C_RED}✗{C_RESET} parser panic: {}\n", msg);
            return (0, 1, 0, true, false);
        }
    };

    let ast = match ast_opt {
        Some(a) => a,
        None => {
            println!("  {C_RED}FAIL{C_RESET} {label}");
            println!("    {C_RED}✗{C_RESET} parse failed\n");
            return (0, 1, 0, true, false);
        }
    };

    let decorator_cases = collect_decorator_tests(&ast);
    if decorator_cases.is_empty() {
        println!("  {C_YELLOW}SKIP{C_RESET} {label}");
        println!("    {C_YELLOW}○{C_RESET} no decorator tests found\n");
        return (0, 0, 1, false, true);
    }

    let errors = Analyzer::analyze(&ast, source);
    let has_lex = !lex.errors.is_empty();
    let has_parse = !parse_errors.is_empty();
    let has_analyzer_err = errors.iter().any(|e| e.severity == Severity::Error);
    let has_any_error = has_lex || has_parse || has_analyzer_err;
    let expected_value_results = if has_any_error {
        HashMap::new()
    } else {
        collect_expected_value_results(&ast, &decorator_cases)
    };

    let mut results: Vec<TestResultItem> = Vec::new();
    for case in &decorator_cases {
        if !case.valid_signature {
            match &case.expectation {
                DecoratorExpectation::Fail(_) => results.push(TestResultItem {
                    name: case.name.clone(),
                    status: TestStatus::Passed,
                    detail: "expected invalid signature failure".to_string(),
                }),
                DecoratorExpectation::Skip => results.push(TestResultItem {
                    name: case.name.clone(),
                    status: TestStatus::Skipped,
                    detail: "marked with #skip".to_string(),
                }),
                DecoratorExpectation::Pass(_) => results.push(TestResultItem {
                    name: case.name.clone(),
                    status: TestStatus::Failed,
                    detail: "signature must be fn test_name() with no params".to_string(),
                }),
            }
            continue;
        }

        match &case.expectation {
            DecoratorExpectation::Skip => {
                results.push(TestResultItem {
                    name: case.name.clone(),
                    status: TestStatus::Skipped,
                    detail: "marked with #skip".to_string(),
                });
            }
            DecoratorExpectation::Pass(expected_opt) => {
                if has_any_error {
                    results.push(TestResultItem {
                        name: case.name.clone(),
                        status: TestStatus::Failed,
                        detail: "compile/analyze failed before running test".to_string(),
                    });
                } else {
                    match expected_opt {
                        None => {
                            results.push(TestResultItem {
                                name: case.name.clone(),
                                status: TestStatus::Passed,
                                detail: String::new(),
                            });
                        }
                        Some(raw_expected) => match parse_expected_value(raw_expected) {
                            Ok(expected) => match expected_value_results.get(&case.name) {
                                Some(Ok(actual)) => {
                                    if values_equal(&expected, actual) {
                                        results.push(TestResultItem {
                                            name: case.name.clone(),
                                            status: TestStatus::Passed,
                                            detail: format!("expected {}", expected),
                                        });
                                    } else {
                                        results.push(TestResultItem {
                                            name: case.name.clone(),
                                            status: TestStatus::Failed,
                                            detail: format!(
                                                "expected {}, got {}",
                                                expected, actual
                                            ),
                                        });
                                    }
                                }
                                Some(Err(e)) => {
                                    results.push(TestResultItem {
                                        name: case.name.clone(),
                                        status: TestStatus::Failed,
                                        detail: format!("expected-value evaluation failed: {}", e),
                                    });
                                }
                                None => {
                                    results.push(TestResultItem {
                                        name: case.name.clone(),
                                        status: TestStatus::Failed,
                                        detail: "expected-value evaluation was not produced"
                                            .to_string(),
                                    });
                                }
                            },
                            Err(e) => {
                                results.push(TestResultItem {
                                    name: case.name.clone(),
                                    status: TestStatus::Failed,
                                    detail: e,
                                });
                            }
                        },
                    }
                }
            }
            DecoratorExpectation::Fail(code_opt) => {
                let ok = if let Some(code) = code_opt {
                    errors.iter().any(|e| e.code == code)
                } else {
                    has_any_error
                };
                if ok {
                    results.push(TestResultItem {
                        name: case.name.clone(),
                        status: TestStatus::Passed,
                        detail: if let Some(code) = code_opt {
                            format!("expected failure {}", code)
                        } else {
                            "expected failure".to_string()
                        },
                    });
                } else {
                    results.push(TestResultItem {
                        name: case.name.clone(),
                        status: TestStatus::Failed,
                        detail: if let Some(code) = code_opt {
                            format!("expected failure {} but it did not occur", code)
                        } else {
                            "expected failure but file compiled cleanly".to_string()
                        },
                    });
                }
            }
        }
    }

    let pass_count = results
        .iter()
        .filter(|r| matches!(r.status, TestStatus::Passed))
        .count();
    let fail_count = results
        .iter()
        .filter(|r| matches!(r.status, TestStatus::Failed))
        .count();
    let skip_count = results
        .iter()
        .filter(|r| matches!(r.status, TestStatus::Skipped))
        .count();

    if fail_count == 0 {
        println!("  {C_GREEN}PASS{C_RESET} {label}");
    } else {
        println!("  {C_RED}FAIL{C_RESET} {label}");
    }

    for r in &results {
        match r.status {
            TestStatus::Passed => {
                if r.detail.is_empty() {
                    println!("    {C_GREEN}✓{C_RESET} {}", r.name);
                } else {
                    println!(
                        "    {C_GREEN}✓{C_RESET} {}  {C_DIM}({}){C_RESET}",
                        r.name, r.detail
                    );
                }
            }
            TestStatus::Failed => {
                println!(
                    "    {C_RED}✕{C_RESET} {}  {C_DIM}({}){C_RESET}",
                    r.name, r.detail
                );
            }
            TestStatus::Skipped => {
                println!(
                    "    {C_YELLOW}○{C_RESET} {}  {C_DIM}({}){C_RESET}",
                    r.name, r.detail
                );
            }
        }
    }

    if fail_count > 0 && has_any_error {
        if has_parse {
            for e in &parse_errors {
                println!("    {C_RED}parse error:{C_RESET} {}", e);
            }
        }
        for e in errors.iter().filter(|e| e.severity == Severity::Error) {
            println!(
                "    {C_RED}analyzer error:{C_RESET} [{C_CYAN}{}{C_RESET}] {}",
                e.code, e.message
            );
        }
    }

    println!(
        "    results: {} passed, {} failed, {} skipped\n",
        pass_count, fail_count, skip_count
    );
    let suite_skipped = pass_count == 0 && fail_count == 0 && skip_count > 0;
    (
        pass_count,
        fail_count,
        skip_count,
        fail_count > 0,
        suite_skipped,
    )
}

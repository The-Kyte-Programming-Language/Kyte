use crate::ast::*;
use std::collections::HashMap;
use std::fmt;

const RED:    &str = "\x1b[1;31m";
const YELLOW: &str = "\x1b[1;33m";
const CYAN:   &str = "\x1b[1;36m";
const DIM:    &str = "\x1b[2m";
const BOLD:   &str = "\x1b[1m";
const RESET:  &str = "\x1b[0m";

#[derive(Clone, Debug, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct CompileError {
    pub code:        &'static str,
    pub severity:    Severity,
    pub message:     String,
    pub hint:        String,
    pub span:        Span,
    pub source_line: String,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (icon, color, label) = match self.severity {
            Severity::Error   => ("\u{2718}", RED, "ERROR"),
            Severity::Warning => ("\u{26A0}", YELLOW, "WARN "),
        };
        writeln!(f, "  {color}{icon} {label} [{code}]{RESET} {BOLD}{msg}{RESET}",
            code = self.code, msg = self.message)?;
        writeln!(f, "     {DIM}\u{2500}\u{2192} line {}:{}{RESET}", self.span.line, self.span.col)?;
        let trimmed = self.source_line.trim_end();
        if !trimmed.is_empty() {
            let leading = trimmed.len() - trimmed.trim_start().len();
            writeln!(f, "      {DIM}\u{2502}{RESET}")?;
            writeln!(f, "  {DIM}{:>3}{RESET} {DIM}\u{2502}{RESET} {}", self.span.line, trimmed)?;
            writeln!(f, "      {DIM}\u{2502}{RESET} {}{color}{}{RESET}",
                " ".repeat(leading),
                "\u{2500}".repeat(trimmed.trim_start().len()))?;
        }
        writeln!(f, "      {CYAN}\u{21B3} hint:{RESET} {DIM}{}{RESET}", self.hint)
    }
}

#[derive(Clone, Debug)]
struct FnSig {
    params:    Vec<Ty>,
    return_ty: Option<Ty>,
}

#[derive(Clone, Debug)]
struct VarInfo {
    ty: Ty,
    #[allow(dead_code)]
    is_vault: bool,
}

fn ty_name(ty: &Ty) -> String {
    match ty {
        Ty::Int    => "int".to_string(),
        Ty::Float  => "float".to_string(),
        Ty::String => "string".to_string(),
        Ty::Bool   => "bool".to_string(),
        Ty::I8     => "i8".to_string(),
        Ty::I16    => "i16".to_string(),
        Ty::I32    => "i32".to_string(),
        Ty::I64    => "i64".to_string(),
        Ty::U8     => "u8".to_string(),
        Ty::U16    => "u16".to_string(),
        Ty::U32    => "u32".to_string(),
        Ty::U64    => "u64".to_string(),
        Ty::Array(inner) => format!("{}[]", ty_name(inner)),
    }
}

fn is_integer_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64
                | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64)
}

fn is_numeric_ty(ty: &Ty) -> bool {
    is_integer_ty(ty) || matches!(ty, Ty::Float)
}

/// 정수 리터럴(Ty::Int)은 모든 정수 타입에 대입 가능
fn types_compatible(expected: &Ty, got: &Ty) -> bool {
    if expected == got { return true; }
    // int literal → 모든 정수 타입 허용
    if *got == Ty::Int && is_integer_ty(expected) { return true; }
    // i64 == int (별칭)
    if (*expected == Ty::Int && *got == Ty::I64) || (*expected == Ty::I64 && *got == Ty::Int) {
        return true;
    }
    false
}

pub struct Analyzer {
    errors:       Vec<CompileError>,
    functions:    HashMap<String, FnSig>,
    source_lines: Vec<String>,
    current_span: Span,
}

impl Analyzer {
    fn err(&mut self, code: &'static str, msg: String, hint: String) {
        let span = self.current_span;
        let source_line = self.source_lines
            .get(span.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        self.errors.push(CompileError {
            code, severity: Severity::Error, message: msg, hint, span, source_line,
        });
    }

    fn warn(&mut self, code: &'static str, msg: String, hint: String) {
        let span = self.current_span;
        let source_line = self.source_lines
            .get(span.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        self.errors.push(CompileError {
            code, severity: Severity::Warning, message: msg, hint, span, source_line,
        });
    }

    pub fn analyze(program: &Program, source: &str) -> Vec<CompileError> {
        let source_lines: Vec<String> = source.lines().map(String::from).collect();
        let mut a = Analyzer {
            errors: Vec::new(),
            functions: HashMap::new(),
            source_lines,
            current_span: Span { line: 0, col: 0 },
        };

        // 0: main 앵커 존재/중복 검사 (top-level)
        let mut main_count = 0usize;
        let mut first_item_span: Option<Span> = None;
        for (item, item_span) in &program.items {
            if first_item_span.is_none() {
                first_item_span = Some(*item_span);
            }
            if let TopLevel::Anchor { kind: AnchorKind::Main, .. } = item {
                main_count += 1;
            }
        }
        if main_count == 0 {
            a.current_span = first_item_span.unwrap_or(Span { line: 1, col: 0 });
            a.err(
                "E018",
                "Missing @...(main) anchor".to_string(),
                "Add a top-level main anchor, e.g. @app(main)".to_string(),
            );
        } else if main_count > 1 {
            a.current_span = first_item_span.unwrap_or(Span { line: 1, col: 0 });
            a.err(
                "E019",
                format!("Multiple main anchors found ({})", main_count),
                "Keep exactly one top-level @...(main) anchor".to_string(),
            );
        }

        // 1: collect function signatures + duplicate check
        for (item, item_span) in &program.items {
            if let TopLevel::Function { name, params, return_ty, .. } = item {
                let sig = FnSig {
                    params: params.iter().map(|p| p.ty.clone()).collect(),
                    return_ty: return_ty.clone(),
                };
                if a.functions.contains_key(name) {
                    a.current_span = *item_span;
                    a.err("E001",
                        format!("Duplicate function '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name));
                } else {
                    a.functions.insert(name.clone(), sig);
                }
            }
        }

        // 2: analyze bodies
        for (item, item_span) in &program.items {
            match item {
                TopLevel::Anchor { .. } => {
                    a.check_anchor(item, *item_span, &HashMap::new());
                }
                TopLevel::Function { name, params, return_ty, body } => {
                    let mut scope: HashMap<String, VarInfo> = HashMap::new();
                    for p in params {
                        scope.insert(p.name.clone(), VarInfo { ty: p.ty.clone(), is_vault: false });
                    }
                    a.check_stmts(body, &mut scope, return_ty.as_ref(), name);
                }
            }
        }

        a.errors
    }

    fn check_anchor(&mut self, anchor: &TopLevel, _anchor_span: Span, inherited: &HashMap<String, VarInfo>) {
        if let TopLevel::Anchor { body, children, .. } = anchor {
            let mut scope = inherited.clone();
            for (stmt, sp) in body {
                self.current_span = *sp;
                self.check_stmt_scoped(stmt, &scope, &None);
                self.collect_decl(stmt, &mut scope);
            }
            for (child, child_span) in children {
                self.check_anchor(child, *child_span, &scope);
            }
        }
    }

    fn check_stmts(
        &mut self,
        stmts: &[(Stmt, Span)],
        scope: &mut HashMap<String, VarInfo>,
        return_ty: Option<&Ty>,
        _ctx: &str,
    ) {
        for (stmt, sp) in stmts {
            self.current_span = *sp;
            self.check_stmt_scoped(stmt, scope, &return_ty);
            self.collect_decl(stmt, scope);
        }
    }

    fn collect_decl(&mut self, stmt: &Stmt, scope: &mut HashMap<String, VarInfo>) {
        match stmt {
            Stmt::VarDecl { ty, name, .. } => {
                if scope.contains_key(name) {
                    self.warn("W001",
                        format!("Variable '{}' shadows a previous declaration", name),
                        format!("Use a different name, or remove the earlier '{}'", name));
                }
                scope.insert(name.clone(), VarInfo { ty: ty.clone(), is_vault: false });
            }
            Stmt::VaultDecl { ty, name, .. } => {
                if scope.contains_key(name) {
                    self.warn("W001",
                        format!("Variable '{}' shadows a previous declaration", name),
                        format!("Use a different name, or remove the earlier '{}'", name));
                }
                scope.insert(name.clone(), VarInfo { ty: ty.clone(), is_vault: true });
            }
            Stmt::For { var, .. } => {
                scope.insert(var.clone(), VarInfo { ty: Ty::Int, is_vault: false });
            }
            _ => {}
        }
    }

    fn check_stmt_scoped(
        &mut self,
        stmt: &Stmt,
        scope: &HashMap<String, VarInfo>,
        return_ty: &Option<&Ty>,
    ) {
        match stmt {
            Stmt::VarDecl { ty, name: _, value } => {
                let val_ty = self.infer_expr(value, scope);
                if let Some(vt) = &val_ty {
                    if !types_compatible(ty, vt) {
                        self.err("E002",
                            format!("Type mismatch \u{2014} expected {}, got {}", ty_name(ty), ty_name(vt)),
                            format!("Change the value to type {}, or declare as {}", ty_name(ty), ty_name(vt)));
                    }
                }
            }
            Stmt::VaultDecl { ty, name: _, value } => {
                let val_ty = self.infer_expr(value, scope);
                if let Some(vt) = &val_ty {
                    if !types_compatible(ty, vt) {
                        self.err("E002",
                            format!("Type mismatch in vault \u{2014} expected {}, got {}", ty_name(ty), ty_name(vt)),
                            format!("Change the value to type {}, or declare vault as {}", ty_name(ty), ty_name(vt)));
                    }
                }
            }
            Stmt::Assign { name, value } => {
                if let Some(info) = scope.get(name) {
                    let val_ty = self.infer_expr(value, scope);
                    if let Some(vt) = &val_ty {
                        if !types_compatible(&info.ty, vt) {
                            self.err("E003",
                                format!("Cannot assign {} to '{}' (declared as {})", ty_name(vt), name, ty_name(&info.ty)),
                                format!("Convert the value to {}, or change '{}' declaration", ty_name(&info.ty), name));
                        }
                    }
                } else {
                    self.err("E004",
                        format!("Undeclared variable '{}'", name),
                        format!("Declare '{}' before use \u{2014} e.g. int {} = ...;", name, name));
                    self.infer_expr(value, scope);
                }
            }
            Stmt::IndexAssign { name, index, value } => {
                if let Some(info) = scope.get(name) {
                    if let Ty::Array(inner) = &info.ty {
                        let idx_ty = self.infer_expr(index, scope);
                        if let Some(ref it) = idx_ty {
                            if *it != Ty::Int {
                                self.err("E014",
                                    format!("Array index must be int, got {}", ty_name(it)),
                                    "Use an integer value for array indexing".into());
                            }
                        }
                        let val_ty = self.infer_expr(value, scope);
                        if let Some(ref vt) = val_ty {
                            if !types_compatible(inner, vt) {
                                self.err("E002",
                                    format!("Type mismatch \u{2014} expected {}, got {}", ty_name(inner), ty_name(vt)),
                                    format!("Array element type is {}", ty_name(inner)));
                            }
                        }
                    } else {
                        self.err("E015",
                            format!("Cannot index into non-array type {}", ty_name(&info.ty)),
                            "Only array types support indexing".into());
                        self.infer_expr(index, scope);
                        self.infer_expr(value, scope);
                    }
                } else {
                    self.err("E004",
                        format!("Undeclared variable '{}'", name),
                        format!("Declare '{}' before use \u{2014} e.g. int[] {} = [...];", name, name));
                    self.infer_expr(index, scope);
                    self.infer_expr(value, scope);
                }
            }
            Stmt::CompoundAssign { name, value, .. } => {
                if let Some(info) = scope.get(name) {
                    let val_ty = self.infer_expr(value, scope);
                    if let Some(vt) = &val_ty {
                        if !types_compatible(&info.ty, vt) {
                            self.err("E003",
                                format!("Cannot compound-assign {} to '{}' (declared as {})", ty_name(vt), name, ty_name(&info.ty)),
                                format!("Ensure the right-hand side is type {}", ty_name(&info.ty)));
                        }
                    }
                } else {
                    self.err("E004",
                        format!("Undeclared variable '{}'", name),
                        format!("Declare '{}' before use \u{2014} e.g. int {} = ...;", name, name));
                    self.infer_expr(value, scope);
                }
            }
            Stmt::Kill(Some(e)) => { self.infer_expr(e, scope); }
            Stmt::Kill(None) | Stmt::Exit | Stmt::Break => {}
            Stmt::Yield(e) => { self.infer_expr(e, scope); }
            Stmt::Return(Some(e)) => {
                let val_ty = self.infer_expr(e, scope);
                if let (Some(expected), Some(got)) = (return_ty, &val_ty) {
                    if !types_compatible(expected, got) {
                        self.err("E005",
                            format!("Return type mismatch \u{2014} expected {}, got {}", ty_name(expected), ty_name(got)),
                            format!("Return a {} value, or change the function signature", ty_name(expected)));
                    }
                }
            }
            Stmt::Return(None) => {
                if let Some(expected) = return_ty {
                    self.err("E005",
                        format!("Return type mismatch \u{2014} expected {}, got nothing", ty_name(expected)),
                        "Add a return value, or change the function to return nothing".into());
                }
            }
            Stmt::If { cond, then_body, else_body } => {
                let cond_ty = self.infer_expr(cond, scope);
                if let Some(ct) = &cond_ty {
                    if *ct != Ty::Bool {
                        self.err("E006",
                            format!("Condition must be bool, got {}", ty_name(ct)),
                            "Use a comparison (e.g. x > 0) or a bool variable".into());
                    }
                }
                let mut then_scope = scope.clone();
                for (s, sp) in then_body {
                    self.current_span = *sp;
                    self.check_stmt_scoped(s, &then_scope, return_ty);
                    self.collect_decl(s, &mut then_scope);
                }
                if let Some(else_body) = else_body {
                    let mut else_scope = scope.clone();
                    for (s, sp) in else_body {
                        self.current_span = *sp;
                        self.check_stmt_scoped(s, &else_scope, return_ty);
                        self.collect_decl(s, &mut else_scope);
                    }
                }
            }
            Stmt::Loop(body) => {
                for (s, _) in body {
                    if let Stmt::VaultDecl { name, .. } = s {
                        let has_free = body.iter().any(|(s2, _)| matches!(s2, Stmt::Free(n) if n == name));
                        if !has_free {
                            self.err("E017",
                                format!("Vault '{}' allocated in loop without explicit free", name),
                                format!("Add free({}) before the loop ends", name));
                        }
                    }
                }
                let mut loop_scope = scope.clone();
                for (s, sp) in body {
                    self.current_span = *sp;
                    self.check_stmt_scoped(s, &loop_scope, return_ty);
                    self.collect_decl(s, &mut loop_scope);
                }
            }
            Stmt::For { from, to, body, var, .. } => {
                self.infer_expr(from, scope);
                self.infer_expr(to, scope);
                for (s, _) in body {
                    if let Stmt::VaultDecl { name, .. } = s {
                        let has_free = body.iter().any(|(s2, _)| matches!(s2, Stmt::Free(n) if n == name));
                        if !has_free {
                            self.err("E017",
                                format!("Vault '{}' allocated in loop without explicit free", name),
                                format!("Add free({}) before the loop ends", name));
                        }
                    }
                }
                let mut for_scope = scope.clone();
                for_scope.insert(var.clone(), VarInfo { ty: Ty::Int, is_vault: false });
                for (s, sp) in body {
                    self.current_span = *sp;
                    self.check_stmt_scoped(s, &for_scope, return_ty);
                    self.collect_decl(s, &mut for_scope);
                }
            }
            Stmt::Free(name) => {
                if !scope.contains_key(name) {
                    self.err("E004",
                        format!("Undeclared variable '{}'", name),
                        format!("Declare '{}' before use \u{2014} e.g. int {} = ...;", name, name));
                }
            }
            Stmt::InlineAnchor { body, .. } => {
                let mut inner_scope = scope.clone();
                for (s, sp) in body {
                    self.current_span = *sp;
                    self.check_stmt_scoped(s, &inner_scope, return_ty);
                    self.collect_decl(s, &mut inner_scope);
                }
            }
            Stmt::ExprStmt(e) => { self.infer_expr(e, scope); }
        }
    }

    fn infer_expr(&mut self, expr: &Expr, scope: &HashMap<String, VarInfo>) -> Option<Ty> {
        match expr {
            Expr::IntLit(_)    => Some(Ty::Int),
            Expr::FloatLit(_)  => Some(Ty::Float),
            Expr::StringLit(_) => Some(Ty::String),
            Expr::Bool(_)      => Some(Ty::Bool),

            Expr::Ident(name) => {
                if let Some(info) = scope.get(name) {
                    Some(info.ty.clone())
                } else {
                    self.err("E004",
                        format!("Undeclared variable '{}'", name),
                        format!("Declare '{}' before use \u{2014} e.g. int {} = ...;", name, name));
                    None
                }
            }

            Expr::UnaryOp { op, expr } => {
                let inner = self.infer_expr(expr, scope);
                match op {
                    UnaryOpKind::Neg => {
                        if let Some(ref t) = inner {
                            if !is_numeric_ty(t) {
                                self.err("E007",
                                    format!("Cannot negate type {}", ty_name(t)),
                                    "Negation (-) only works on numeric types".into());
                            }
                        }
                        inner
                    }
                    UnaryOpKind::Not => {
                        if let Some(ref t) = inner {
                            if *t != Ty::Bool {
                                self.err("E007",
                                    format!("Cannot apply '!' to type {}", ty_name(t)),
                                    "Logical not (!) only works on bool values".into());
                            }
                        }
                        Some(Ty::Bool)
                    }
                }
            }

            Expr::BinOp { left, op, right } => {
                let lt = self.infer_expr(left, scope);
                let rt = self.infer_expr(right, scope);

                match op {
                    BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul
                    | BinOpKind::Div | BinOpKind::Mod => {
                        if matches!(op, BinOpKind::Add) {
                            if matches!((&lt, &rt), (Some(Ty::String), _) | (_, Some(Ty::String))) {
                                return Some(Ty::String);
                            }
                        }
                        if let (Some(ref l), Some(ref r)) = (&lt, &rt) {
                            if !types_compatible(l, r) && !types_compatible(r, l) {
                                self.err("E008",
                                    format!("Arithmetic type mismatch \u{2014} {} vs {}", ty_name(l), ty_name(r)),
                                    "Both sides of an arithmetic operation must be the same numeric type".into());
                            }
                            if !is_numeric_ty(l) {
                                self.err("E008",
                                    format!("Arithmetic on non-numeric type {}", ty_name(l)),
                                    "Arithmetic operators (+, -, *, /, %) only work on numeric types".into());
                            }
                        }
                        lt
                    }
                    BinOpKind::Lt | BinOpKind::Gt | BinOpKind::Le
                    | BinOpKind::Ge | BinOpKind::Eq | BinOpKind::Neq => {
                        if let (Some(ref l), Some(ref r)) = (&lt, &rt) {
                            if !types_compatible(l, r) && !types_compatible(r, l) {
                                self.err("E009",
                                    format!("Comparison type mismatch \u{2014} {} vs {}", ty_name(l), ty_name(r)),
                                    "Both sides of a comparison must be the same type".into());
                            }
                        }
                        Some(Ty::Bool)
                    }
                    BinOpKind::And | BinOpKind::Or => {
                        if let Some(ref l) = lt {
                            if *l != Ty::Bool {
                                self.err("E010",
                                    format!("Logical operator requires bool, got {}", ty_name(l)),
                                    "Use a comparison (e.g. x > 0) to produce a bool".into());
                            }
                        }
                        if let Some(ref r) = rt {
                            if *r != Ty::Bool {
                                self.err("E010",
                                    format!("Logical operator requires bool, got {}", ty_name(r)),
                                    "Use a comparison (e.g. x > 0) to produce a bool".into());
                            }
                        }
                        Some(Ty::Bool)
                    }
                }
            }

            Expr::Call { name, args } => {
                // len() 빌트인
                if name == "len" {
                    if args.len() != 1 {
                        self.err("E011",
                            format!("'len' expects 1 argument, got {}", args.len()),
                            "Usage: len(array)".into());
                    } else {
                        let arg_ty = self.infer_expr(&args[0], scope);
                        if let Some(ref at) = arg_ty {
                            if !matches!(at, Ty::Array(_)) {
                                self.err("E015",
                                    format!("'len' expects an array, got {}", ty_name(at)),
                                    "Pass an array variable to len()".into());
                            }
                        }
                    }
                    return Some(Ty::Int);
                }

                if let Some(sig) = self.functions.get(name).cloned() {
                    if args.len() != sig.params.len() {
                        self.err("E011",
                            format!("'{}' expects {} argument(s), got {}", name, sig.params.len(), args.len()),
                            format!("Signature: {}({})", name,
                                sig.params.iter().map(|p| ty_name(p)).collect::<Vec<_>>().join(", ")));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = self.infer_expr(arg, scope);
                        if let (Some(ref at), Some(expected)) = (&arg_ty, sig.params.get(i)) {
                            if !types_compatible(expected, at) {
                                self.err("E012",
                                    format!("Arg {} of '{}' \u{2014} expected {}, got {}", i + 1, name, ty_name(expected), ty_name(at)),
                                    format!("Pass a {} value as argument {}", ty_name(expected), i + 1));
                            }
                        }
                    }
                    sig.return_ty
                } else {
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    self.err("E013",
                        format!("Undeclared function '{}'", name),
                        format!("Define '{}' with fn {}(...) {{ ... }}", name, name));
                    None
                }
            }

            Expr::ArrayLit(elems) => {
                if elems.is_empty() {
                    self.err("E016",
                        "Empty array literal \u{2014} cannot infer element type".to_string(),
                        "Provide at least one element, e.g. [0]".into());
                    return None;
                }
                let first_ty = self.infer_expr(&elems[0], scope);
                for (i, elem) in elems.iter().enumerate().skip(1) {
                    let elem_ty = self.infer_expr(elem, scope);
                    if let (Some(ref ft), Some(ref et)) = (&first_ty, &elem_ty) {
                        if ft != et {
                            self.err("E002",
                                format!("Array element {} has type {}, expected {}", i, ty_name(et), ty_name(ft)),
                                "All array elements must be the same type".into());
                        }
                    }
                }
                first_ty.map(|t| Ty::Array(Box::new(t)))
            }

            Expr::Index { array, index } => {
                let arr_ty = self.infer_expr(array, scope);
                let idx_ty = self.infer_expr(index, scope);
                if let Some(ref it) = idx_ty {
                    if *it != Ty::Int {
                        self.err("E014",
                            format!("Array index must be int, got {}", ty_name(it)),
                            "Use an integer value for array indexing".into());
                    }
                }
                if let Some(Ty::Array(inner)) = arr_ty {
                    Some(*inner)
                } else {
                    if let Some(ref t) = arr_ty {
                        self.err("E015",
                            format!("Cannot index into non-array type {}", ty_name(t)),
                            "Only array types support indexing".into());
                    }
                    None
                }
            }
        }
    }
}
use crate::ast::*;
use std::collections::{HashMap, HashSet};
#[path = "analyzer/shared.rs"]
mod shared;

use shared::{
    is_integer_ty, is_numeric_ty, nearest_name, ty_name, types_compatible, FnSig, VarInfo,
};
pub use shared::{CompileError, Severity};

pub struct Analyzer {
    errors: Vec<CompileError>,
    functions: HashMap<String, FnSig>,
    structs: HashMap<String, Vec<StructField>>,
    enums: HashMap<String, Vec<crate::ast::EnumVariant>>,
    source_lines: Vec<String>,
    current_span: Span,
    in_anchor: bool,
    /// 사용된 변수 추적 (unused variable 경고용)
    used_vars: HashSet<String>,
    /// 선언된 변수 (이름, span)
    declared_vars: Vec<(String, Span)>,
    /// 분석 설정 (A05)
    config: AnalyzerConfig,
    /// 최상위 const 선언 (전역 상수)
    global_consts: HashMap<String, VarInfo>,
    /// 등록된 모듈 이름 (mod call 해석용)
    module_names: HashSet<String>,
}

/// Analyzer 동작을 제어하는 설정 구조체 (A05)
#[derive(Clone, Debug, Default)]
pub struct AnalyzerConfig {
    /// 모든 경고 활성화 (--Wall)
    pub wall: bool,
    /// 경고를 오류로 처리 (--Werror)
    pub werror: bool,
    /// 미사용 변수 경고 비활성화 (--no-unused)
    pub no_unused: bool,
}

fn int_range(ty: &Ty) -> Option<(i128, i128)> {
    match ty {
        Ty::I8 => Some((-128, 127)),
        Ty::I16 => Some((-32768, 32767)),
        Ty::I32 => Some((-2147483648, 2147483647)),
        Ty::I64 | Ty::Int => Some((-9223372036854775808, 9223372036854775807)),
        Ty::U8 => Some((0, 255)),
        Ty::U16 => Some((0, 65535)),
        Ty::U32 => Some((0, 4294967295)),
        Ty::U64 => Some((0, 18446744073709551615)),
        _ => None,
    }
}

fn const_int_value(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::IntLit(n) => Some(*n as i128),
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr,
        } => const_int_value(expr).map(|v| -v),
        _ => None,
    }
}

impl Analyzer {
    fn stmt_is_early_exit(stmt: &Stmt) -> bool {
        matches!(
            stmt,
            Stmt::Return(_) | Stmt::Exit | Stmt::Break | Stmt::Kill(_) | Stmt::Yield(_)
        )
    }

    fn block_has_break(stmts: &[(Stmt, Span)]) -> bool {
        for (stmt, _) in stmts {
            match stmt {
                Stmt::Break => return true,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    if Self::block_has_break(then_body) {
                        return true;
                    }
                    if let Some(eb) = else_body {
                        if Self::block_has_break(eb) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// 블록이 반드시 일찍 종료(return/exit/break/kill/yield)되는지 확인
    fn block_definitely_exits(stmts: &[(Stmt, Span)]) -> bool {
        for (stmt, _) in stmts {
            if Self::stmt_is_early_exit(stmt) {
                return true;
            }
            // if/else 양쪽 모두 exit하면 전체가 exit
            if let Stmt::If {
                then_body,
                else_body: Some(else_body),
                ..
            } = stmt
            {
                if Self::block_definitely_exits(then_body)
                    && Self::block_definitely_exits(else_body)
                {
                    return true;
                }
            }
        }
        false
    }

    fn collect_decl_with_log(
        &mut self,
        stmt: &Stmt,
        scope: &mut HashMap<String, VarInfo>,
        scope_log: &mut Vec<(String, Option<VarInfo>)>,
    ) {
        match stmt {
            Stmt::VarDecl { ty, name, .. } => {
                if scope.contains_key(name) {
                    self.warn(
                        "W001",
                        format!("Variable '{}' shadows a previous declaration", name),
                        format!("Use a different name, or remove the earlier '{}'", name),
                    );
                }
                scope_log.push((name.clone(), scope.get(name).cloned()));
                scope.insert(
                    name.clone(),
                    VarInfo {
                        ty: ty.clone(),
                        is_vault: false,
                    },
                );
            }
            Stmt::VaultDecl { ty, name, .. } => {
                if scope.contains_key(name) {
                    self.warn(
                        "W001",
                        format!("Variable '{}' shadows a previous declaration", name),
                        format!("Use a different name, or remove the earlier '{}'", name),
                    );
                }
                scope_log.push((name.clone(), scope.get(name).cloned()));
                scope.insert(
                    name.clone(),
                    VarInfo {
                        ty: ty.clone(),
                        is_vault: true,
                    },
                );
            }
            Stmt::For { var, .. } => {
                scope_log.push((var.clone(), scope.get(var).cloned()));
                scope.insert(
                    var.clone(),
                    VarInfo {
                        ty: Ty::Int,
                        is_vault: false,
                    },
                );
            }
            _ => {}
        }
    }

    fn restore_scope(
        scope: &mut HashMap<String, VarInfo>,
        scope_log: Vec<(String, Option<VarInfo>)>,
    ) {
        for (name, previous) in scope_log.into_iter().rev() {
            match previous {
                Some(info) => {
                    scope.insert(name, info);
                }
                None => {
                    scope.remove(&name);
                }
            }
        }
    }

    fn check_scoped_block(
        &mut self,
        stmts: &[(Stmt, Span)],
        scope: &mut HashMap<String, VarInfo>,
        return_ty: &Option<&Ty>,
        _enforce_loop_vault_free: bool,
    ) {
        let mut scope_log = Vec::new();
        // H07: track decl spans for unused variable warning
        let mut decl_spans: HashMap<String, Span> = HashMap::new();
        let mut found_exit = false;
        for (idx, (stmt, sp)) in stmts.iter().enumerate() {
            self.current_span = *sp;
            // H07: unreachable code warning
            if found_exit && idx > 0 {
                self.warn(
                    "W004",
                    "Unreachable code after return/exit/break/kill".to_string(),
                    "Remove or move the code before the exit point".to_string(),
                );
                break;
            }
            if Self::stmt_is_early_exit(stmt) {
                found_exit = true;
            }
            self.check_stmt_scoped(stmt, scope, return_ty);
            // H07: track new decls
            match stmt {
                Stmt::VarDecl { name, .. } | Stmt::VaultDecl { name, .. } => {
                    decl_spans.entry(name.clone()).or_insert(*sp);
                }
                Stmt::For { var, .. } => {
                    decl_spans.entry(var.clone()).or_insert(*sp);
                }
                _ => {}
            }
            self.collect_decl_with_log(stmt, scope, &mut scope_log);
        }

        // H07: unused variable 경고 (명칭이 _로 시작하면 suppress, --no-unused이면 skip)
        if !self.config.no_unused {
            for (name, sp) in &decl_spans {
                if !name.starts_with('_') && !self.used_vars.contains(name) {
                    self.current_span = *sp;
                    self.warn(
                        "W003",
                        format!("Variable '{}' is declared but never used", name),
                        format!("Prefix with '_' to suppress: _{}", name),
                    );
                }
            }
        }

        Self::restore_scope(scope, scope_log);
    }

    fn err(&mut self, code: &'static str, msg: String, hint: String) {
        let span = self.current_span;
        let source_line = self
            .source_lines
            .get(span.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        self.errors.push(CompileError {
            code,
            severity: Severity::Error,
            message: msg,
            hint,
            span,
            source_line,
        });
    }

    fn warn(&mut self, code: &'static str, msg: String, hint: String) {
        let span = self.current_span;
        let source_line = self
            .source_lines
            .get(span.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        // --Werror: 경고를 오류로 승격 (A05)
        let severity = if self.config.werror {
            Severity::Error
        } else {
            Severity::Warning
        };
        self.errors.push(CompileError {
            code,
            severity,
            message: msg,
            hint,
            span,
            source_line,
        });
    }

    fn err_undeclared_var(&mut self, name: &str, scope: &HashMap<String, VarInfo>) {
        let hint = if let Some(similar) = nearest_name(name, scope.keys()) {
            format!("Did you mean '{}' ?", similar)
        } else {
            format!("Declare '{}' before use, e.g. int {} = ...;", name, name)
        };
        self.err("E004", format!("Undeclared variable '{}'", name), hint);
    }

    fn err_undeclared_fn(&mut self, name: &str) {
        let hint = if let Some(similar) = nearest_name(name, self.functions.keys()) {
            format!("Did you mean '{}' ?", similar)
        } else {
            format!("Define '{}' with fn {}(...) {{ ... }}", name, name)
        };
        self.err("E013", format!("Undeclared function '{}'", name), hint);
    }

    pub fn analyze(program: &Program, source: &str) -> Vec<CompileError> {
        Self::analyze_with_config(program, source, AnalyzerConfig::default())
    }

    pub fn analyze_with_config(
        program: &Program,
        source: &str,
        config: AnalyzerConfig,
    ) -> Vec<CompileError> {
        let source_lines: Vec<String> = source.lines().map(String::from).collect();
        let mut a = Analyzer {
            errors: Vec::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            source_lines,
            current_span: Span { line: 0, col: 0 },
            in_anchor: false,
            used_vars: HashSet::new(),
            declared_vars: Vec::new(),
            config,
            global_consts: HashMap::new(),
            module_names: HashSet::new(),
        };

        // 0: verify top-level main anchor existence/uniqueness
        let mut main_count = 0usize;
        let mut first_item_span: Option<Span> = None;
        for (item, item_span) in &program.items {
            if first_item_span.is_none() {
                first_item_span = Some(*item_span);
            }
            if let TopLevel::Anchor {
                kind: AnchorKind::Main,
                ..
            } = item
            {
                main_count += 1;
            }
            if let TopLevel::Anchor {
                name,
                kind: AnchorKind::Main,
                ..
            } = item
            {
                if name != "main" {
                    a.current_span = *item_span;
                    a.err(
                        "E022",
                        format!(
                            "Main anchor name must be '@main(main)', got '@{}(main)'",
                            name
                        ),
                        "Rename the anchor to @main(main)".to_string(),
                    );
                }
            }
        }
        if main_count == 0 {
            a.current_span = first_item_span.unwrap_or(Span { line: 1, col: 0 });
            a.err(
                "E018",
                "Missing @...(main) anchor".to_string(),
                "Add a top-level main anchor, e.g. @main(main)".to_string(),
            );
        } else if main_count > 1 {
            a.current_span = first_item_span.unwrap_or(Span { line: 1, col: 0 });
            a.err(
                "E019",
                format!("Multiple main anchors found ({})", main_count),
                "Keep exactly one top-level @...(main) anchor".to_string(),
            );
        }

        // 1: collect function signatures + struct/enum definitions + duplicate check
        for (item, item_span) in &program.items {
            if let TopLevel::Struct { name, fields } = item {
                if a.structs.contains_key(name) {
                    a.current_span = *item_span;
                    a.err(
                        "E025",
                        format!("Duplicate struct '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name),
                    );
                } else {
                    a.structs.insert(name.clone(), fields.clone());
                }
            }
            if let TopLevel::Enum { name, variants } = item {
                if a.enums.contains_key(name) {
                    a.current_span = *item_span;
                    a.err(
                        "E025",
                        format!("Duplicate enum '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name),
                    );
                } else {
                    a.enums.insert(name.clone(), variants.clone());
                }
            }
            if let TopLevel::Function {
                name,
                params,
                return_ty,
                ..
            } = item
            {
                let sig = FnSig {
                    params: params.iter().map(|p| p.ty.clone()).collect(),
                    return_ty: return_ty.clone(),
                };
                if a.functions.contains_key(name) {
                    a.current_span = *item_span;
                    a.err(
                        "E001",
                        format!("Duplicate function '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name),
                    );
                } else {
                    a.functions.insert(name.clone(), sig);
                }
            }
            // Impl 블록의 메서드를 TraitName_TypeName_method 형식으로 등록
            if let TopLevel::Impl {
                trait_name,
                target_ty,
                methods,
            } = item
            {
                for (method_tl, _) in methods {
                    if let TopLevel::Function {
                        name: mname,
                        params,
                        return_ty,
                        ..
                    } = method_tl
                    {
                        let qualified = format!("{}_{}_{}", trait_name, target_ty, mname);
                        let sig = FnSig {
                            params: params.iter().map(|p| p.ty.clone()).collect(),
                            return_ty: return_ty.clone(),
                        };
                        a.functions.insert(qualified, sig);
                    }
                }
            }
            // Module 내부의 함수를 modname_funcname 형식으로 등록
            if let TopLevel::Module {
                name: modname,
                items: mod_items,
            } = item
            {
                a.module_names.insert(modname.clone());
                for (mod_tl, _) in mod_items {
                    if let TopLevel::Function {
                        name: fn_name,
                        params,
                        return_ty,
                        ..
                    } = mod_tl
                    {
                        let qualified = format!("{}_{}", modname, fn_name);
                        let sig = FnSig {
                            params: params.iter().map(|p| p.ty.clone()).collect(),
                            return_ty: return_ty.clone(),
                        };
                        a.functions.insert(qualified, sig);
                    }
                }
            }
            // 최상위 const 선언을 global_consts에 등록
            if let TopLevel::ConstDecl { ty, name, value } = item {
                let effective_ty = if *ty == Ty::Auto {
                    // 간단한 추론: IntLit → Int, FloatLit → Float, StringLit → String, Bool → Bool
                    match value {
                        Expr::IntLit(_) => Ty::Int,
                        Expr::FloatLit(_) => Ty::Float,
                        Expr::StringLit(_) => Ty::String,
                        Expr::Bool(_) => Ty::Bool,
                        _ => Ty::Int,
                    }
                } else {
                    ty.clone()
                };
                a.global_consts.insert(
                    name.clone(),
                    VarInfo {
                        ty: effective_ty,
                        is_vault: false,
                    },
                );
            }
        }

        // H06: 순환 참조 구조체 감지
        for (item, item_span) in &program.items {
            if let TopLevel::Struct { name, .. } = item {
                let mut visiting = HashSet::new();
                if a.check_circular_struct(name, &mut visiting) {
                    a.current_span = *item_span;
                    a.err(
                        "E034",
                        format!("Circular reference detected in struct '{}'", name),
                        "Use a pointer/Vault or break the cycle".to_string(),
                    );
                }
            }
        }

        // 2: analyze bodies
        for (item, item_span) in &program.items {
            match item {
                TopLevel::Anchor { .. } => {
                    a.check_anchor(item, *item_span, &HashMap::new());
                }
                TopLevel::Function {
                    name,
                    params,
                    return_ty,
                    body,
                    ..
                } => {
                    let mut scope: HashMap<String, VarInfo> = HashMap::new();
                    for p in params {
                        scope.insert(
                            p.name.clone(),
                            VarInfo {
                                ty: p.ty.clone(),
                                is_vault: false,
                            },
                        );
                    }
                    a.check_stmts(body, &mut scope, return_ty.as_ref(), name);
                    // H04: 반환값이 있는 함수의 모든 경로에 return이 있는지 검사
                    if return_ty.is_some() && !Self::block_definitely_exits(body) {
                        a.current_span = *item_span;
                        a.err(
                            "E033",
                            format!("Function '{}' may not return a value on all paths", name),
                            "Ensure every code path has a return statement".to_string(),
                        );
                    }
                    // H07: 미사용 변수 경고
                    for p in params {
                        a.declared_vars.push((p.name.clone(), *item_span));
                    }
                }
                TopLevel::Struct { .. } => {}
                TopLevel::Enum { .. } => {}
                TopLevel::Trait { .. } => {} // trait은 선언만, 본문 없음
                TopLevel::Impl {
                    trait_name,
                    target_ty,
                    methods,
                } => {
                    // impl 메서드도 일반 함수처럼 분석
                    for (method_tl, _method_span) in methods {
                        if let TopLevel::Function {
                            name: mname,
                            params,
                            return_ty,
                            body,
                            ..
                        } = method_tl
                        {
                            let qualified = format!("{}_{}_{}", trait_name, target_ty, mname);
                            let mut scope: HashMap<String, VarInfo> = HashMap::new();
                            for p in params {
                                scope.insert(
                                    p.name.clone(),
                                    VarInfo {
                                        ty: p.ty.clone(),
                                        is_vault: false,
                                    },
                                );
                            }
                            a.check_stmts(body, &mut scope, return_ty.as_ref(), &qualified);
                        }
                    }
                }
                TopLevel::Module {
                    name: modname,
                    items: mod_items,
                } => {
                    // module 내부 함수 분석
                    for (mod_tl, _mod_span) in mod_items {
                        if let TopLevel::Function {
                            name: fn_name,
                            params,
                            return_ty,
                            body,
                            ..
                        } = mod_tl
                        {
                            let qualified = format!("{}_{}", modname, fn_name);
                            let mut scope: HashMap<String, VarInfo> = HashMap::new();
                            for p in params {
                                scope.insert(
                                    p.name.clone(),
                                    VarInfo {
                                        ty: p.ty.clone(),
                                        is_vault: false,
                                    },
                                );
                            }
                            a.check_stmts(body, &mut scope, return_ty.as_ref(), &qualified);
                        }
                    }
                }
                TopLevel::ConstDecl { .. } => {} // 최상위 const는 값 체크 생략 (간단히 통과)
            }
        }

        a.errors
    }

    fn check_anchor(
        &mut self,
        anchor: &TopLevel,
        _anchor_span: Span,
        inherited: &HashMap<String, VarInfo>,
    ) {
        if let TopLevel::Anchor {
            kind,
            body,
            children,
            ..
        } = anchor
        {
            if matches!(kind, AnchorKind::Thread | AnchorKind::Event(_)) {
                self.err(
                    "E024",
                    format!("Anchor kind '{:?}' is parsed but not implemented", kind),
                    "Use plain @name() or @main(main) until runtime semantics are implemented"
                        .to_string(),
                );
            }
            let saved = self.in_anchor;
            self.in_anchor = true;
            let mut scope = inherited.clone();
            // 전역 상수를 앵커 스코프에 주입
            let global_consts = self.global_consts.clone();
            for (name, info) in &global_consts {
                scope.entry(name.clone()).or_insert_with(|| info.clone());
            }
            for (stmt, sp) in body {
                self.current_span = *sp;
                self.check_stmt_scoped(stmt, &mut scope, &None);
                self.collect_decl(stmt, &mut scope);
            }
            for (child, child_span) in children {
                self.check_anchor(child, *child_span, &scope);
            }
            self.in_anchor = saved;
        }
    }

    fn check_stmts(
        &mut self,
        stmts: &[(Stmt, Span)],
        scope: &mut HashMap<String, VarInfo>,
        return_ty: Option<&Ty>,
        _ctx: &str,
    ) {
        // 전역 상수를 스코프에 주입 (함수 내 로컬 변수가 없는 경우에만)
        let global_consts = self.global_consts.clone();
        for (name, info) in &global_consts {
            scope.entry(name.clone()).or_insert_with(|| info.clone());
        }
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
                    self.warn(
                        "W001",
                        format!("Variable '{}' shadows a previous declaration", name),
                        format!("Use a different name, or remove the earlier '{}'", name),
                    );
                }
                scope.insert(
                    name.clone(),
                    VarInfo {
                        ty: ty.clone(),
                        is_vault: false,
                    },
                );
            }
            Stmt::VaultDecl { ty, name, .. } => {
                if scope.contains_key(name) {
                    self.warn(
                        "W001",
                        format!("Variable '{}' shadows a previous declaration", name),
                        format!("Use a different name, or remove the earlier '{}'", name),
                    );
                }
                scope.insert(
                    name.clone(),
                    VarInfo {
                        ty: ty.clone(),
                        is_vault: true,
                    },
                );
            }
            Stmt::For { var, .. } => {
                scope.insert(
                    var.clone(),
                    VarInfo {
                        ty: Ty::Int,
                        is_vault: false,
                    },
                );
            }
            _ => {}
        }
    }

    fn check_stmt_scoped(
        &mut self,
        stmt: &Stmt,
        scope: &mut HashMap<String, VarInfo>,
        return_ty: &Option<&Ty>,
    ) {
        match stmt {
            Stmt::VarDecl { ty, name, value } => {
                // A07: auto 타입 추론
                let effective_ty = if *ty == Ty::Auto {
                    let inferred = self.infer_expr(value, scope);
                    if let Some(ref vt) = inferred {
                        // scope 업데이트
                        if let Some(info) = scope.get_mut(name) {
                            info.ty = vt.clone();
                        }
                        vt.clone()
                    } else {
                        self.err(
                            "E035",
                            "Cannot infer type for 'auto' declaration".to_string(),
                            "Specify the type explicitly".to_string(),
                        );
                        Ty::Int // fallback
                    }
                } else {
                    ty.clone()
                };

                if let Ty::Struct(sname) = &effective_ty {
                    if !self.structs.contains_key(sname) {
                        self.err(
                            "E026",
                            format!("Unknown struct type '{}'", sname),
                            format!("Declare 'struct {} {{ ... }}' before using it", sname),
                        );
                    }
                }
                if *ty != Ty::Auto {
                    let val_ty = self.infer_expr(value, scope);
                    if let Some(vt) = &val_ty {
                        if !types_compatible(&effective_ty, vt) {
                            self.err(
                                "E002",
                                format!(
                                    "Type mismatch \u{2014} expected {}, got {}",
                                    ty_name(&effective_ty),
                                    ty_name(vt)
                                ),
                                format!(
                                    "Change the value to type {}, or declare as {}",
                                    ty_name(&effective_ty),
                                    ty_name(vt)
                                ),
                            );
                        }
                    }
                }
                // H03: 정수 범위 체크
                self.check_int_range(&effective_ty, value);
            }
            Stmt::VaultDecl { ty, name: _, value } => {
                if let Ty::Struct(sname) = ty {
                    if !self.structs.contains_key(sname) {
                        self.err(
                            "E026",
                            format!("Unknown struct type '{}'", sname),
                            format!("Declare 'struct {} {{ ... }}' before using it", sname),
                        );
                    }
                }
                let val_ty = self.infer_expr(value, scope);
                if let Some(vt) = &val_ty {
                    if !types_compatible(ty, vt) {
                        self.err(
                            "E002",
                            format!(
                                "Type mismatch in vault \u{2014} expected {}, got {}",
                                ty_name(ty),
                                ty_name(vt)
                            ),
                            format!(
                                "Change the value to type {}, or declare vault as {}",
                                ty_name(ty),
                                ty_name(vt)
                            ),
                        );
                    }
                }
                // H03: 정수 범위 체크
                self.check_int_range(ty, value);
            }
            Stmt::Assign { name, value } => {
                if let Some(info) = scope.get(name) {
                    let val_ty = self.infer_expr(value, scope);
                    if let Some(vt) = &val_ty {
                        if !types_compatible(&info.ty, vt) {
                            self.err(
                                "E003",
                                format!(
                                    "Cannot assign {} to '{}' (declared as {})",
                                    ty_name(vt),
                                    name,
                                    ty_name(&info.ty)
                                ),
                                format!(
                                    "Convert the value to {}, or change '{}' declaration",
                                    ty_name(&info.ty),
                                    name
                                ),
                            );
                        }
                    }
                } else {
                    self.err(
                        "E004",
                        format!("Undeclared variable '{}'", name),
                        format!(
                            "Declare '{}' before use \u{2014} e.g. int {} = ...;",
                            name, name
                        ),
                    );
                    self.infer_expr(value, scope);
                }
            }
            Stmt::IndexAssign { name, index, value } => {
                if let Some(info) = scope.get(name) {
                    if let Ty::Array(inner) = &info.ty {
                        let idx_ty = self.infer_expr(index, scope);
                        if let Some(ref it) = idx_ty {
                            if *it != Ty::Int {
                                self.err(
                                    "E014",
                                    format!("Array index must be int, got {}", ty_name(it)),
                                    "Use an integer value for array indexing".into(),
                                );
                            }
                        }
                        let val_ty = self.infer_expr(value, scope);
                        if let Some(ref vt) = val_ty {
                            if !types_compatible(inner, vt) {
                                self.err(
                                    "E002",
                                    format!(
                                        "Type mismatch \u{2014} expected {}, got {}",
                                        ty_name(inner),
                                        ty_name(vt)
                                    ),
                                    format!("Array element type is {}", ty_name(inner)),
                                );
                            }
                        }
                    } else {
                        self.err(
                            "E015",
                            format!("Cannot index into non-array type {}", ty_name(&info.ty)),
                            "Only array types support indexing".into(),
                        );
                        self.infer_expr(index, scope);
                        self.infer_expr(value, scope);
                    }
                } else {
                    self.err(
                        "E004",
                        format!("Undeclared variable '{}'", name),
                        format!(
                            "Declare '{}' before use \u{2014} e.g. int[] {} = [...];",
                            name, name
                        ),
                    );
                    self.infer_expr(index, scope);
                    self.infer_expr(value, scope);
                }
            }
            Stmt::FieldAssign { name, field, value } => {
                if let Some(info) = scope.get(name) {
                    if let Ty::Struct(sname) = &info.ty {
                        if let Some(fields) = self.structs.get(sname) {
                            if let Some(sf) = fields.iter().find(|f| f.name == *field) {
                                let expected_ty = sf.ty.clone();
                                let val_ty = self.infer_expr(value, scope);
                                if let Some(vt) = &val_ty {
                                    if !types_compatible(&expected_ty, vt) {
                                        self.err(
                                            "E002",
                                            format!(
                                                "Type mismatch for field '{}.{}' — expected {}, got {}",
                                                name,
                                                field,
                                                ty_name(&expected_ty),
                                                ty_name(vt)
                                            ),
                                            format!("Field '{}.{}' type is {}", name, field, ty_name(&expected_ty)),
                                        );
                                    }
                                }
                            } else {
                                self.err(
                                    "E027",
                                    format!("Struct '{}' has no field '{}'", sname, field),
                                    "Check the struct declaration for available fields".to_string(),
                                );
                                self.infer_expr(value, scope);
                            }
                        }
                    } else {
                        self.err(
                            "E028",
                            format!(
                                "Cannot assign field '{}.{}' on non-struct type {}",
                                name,
                                field,
                                ty_name(&info.ty)
                            ),
                            "Field assignment requires a struct variable".to_string(),
                        );
                        self.infer_expr(value, scope);
                    }
                } else {
                    self.err_undeclared_var(name, scope);
                    self.infer_expr(value, scope);
                }
            }
            Stmt::CompoundAssign { name, value, .. } => {
                if let Some(info) = scope.get(name) {
                    let val_ty = self.infer_expr(value, scope);
                    if let Some(vt) = &val_ty {
                        if !types_compatible(&info.ty, vt) {
                            self.err(
                                "E003",
                                format!(
                                    "Cannot compound-assign {} to '{}' (declared as {})",
                                    ty_name(vt),
                                    name,
                                    ty_name(&info.ty)
                                ),
                                format!("Ensure the right-hand side is type {}", ty_name(&info.ty)),
                            );
                        }
                    }
                } else {
                    self.err(
                        "E004",
                        format!("Undeclared variable '{}'", name),
                        format!(
                            "Declare '{}' before use \u{2014} e.g. int {} = ...;",
                            name, name
                        ),
                    );
                    self.infer_expr(value, scope);
                }
            }
            Stmt::Kill(Some(e)) => {
                if !self.in_anchor {
                    self.err(
                        "E023",
                        "'Kill' can only be used inside an anchor".to_string(),
                        "Move this Kill into an @anchor() { ... } block".to_string(),
                    );
                }
                self.infer_expr(e, scope);
            }
            Stmt::Kill(None) => {
                if !self.in_anchor {
                    self.err(
                        "E023",
                        "'Kill' can only be used inside an anchor".to_string(),
                        "Move this Kill into an @anchor() { ... } block".to_string(),
                    );
                }
            }
            Stmt::Exit | Stmt::Break => {}
            Stmt::Yield(e) => {
                if !self.in_anchor {
                    self.err(
                        "E020",
                        "'yield' can only be used inside an anchor".to_string(),
                        "Move this yield into an @anchor() { ... } block".to_string(),
                    );
                }
                self.infer_expr(e, scope);
            }
            Stmt::Print(args) => {
                for a in args {
                    self.infer_expr(a, scope);
                }
            }
            Stmt::Assert { cond, message } => {
                let cond_ty = self.infer_expr(cond, scope);
                if let Some(ct) = &cond_ty {
                    if *ct != Ty::Bool {
                        self.err(
                            "E036",
                            format!("assert condition must be bool, got {}", ty_name(ct)),
                            "Use a boolean expression as the assert condition".to_string(),
                        );
                    }
                }
                if let Some(msg) = message {
                    self.infer_expr(msg, scope);
                }
            }
            Stmt::Return(Some(e)) => {
                let val_ty = self.infer_expr(e, scope);
                if let (Some(expected), Some(got)) = (return_ty, &val_ty) {
                    if !types_compatible(expected, got) {
                        self.err(
                            "E005",
                            format!(
                                "Return type mismatch \u{2014} expected {}, got {}",
                                ty_name(expected),
                                ty_name(got)
                            ),
                            format!(
                                "Return a {} value, or change the function signature",
                                ty_name(expected)
                            ),
                        );
                    }
                }
            }
            Stmt::Return(None) => {
                if let Some(expected) = return_ty {
                    self.err(
                        "E005",
                        format!(
                            "Return type mismatch \u{2014} expected {}, got nothing",
                            ty_name(expected)
                        ),
                        "Add a return value, or change the function to return nothing".into(),
                    );
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let cond_ty = self.infer_expr(cond, scope);
                if let Some(ct) = &cond_ty {
                    if *ct != Ty::Bool {
                        self.err(
                            "E006",
                            format!("Condition must be bool, got {}", ty_name(ct)),
                            "Use a comparison (e.g. x > 0) or a bool variable".into(),
                        );
                    }
                }
                self.check_scoped_block(then_body, scope, return_ty, false);
                if let Some(else_body) = else_body {
                    self.check_scoped_block(else_body, scope, return_ty, false);
                }
            }
            Stmt::Loop(body) => {
                // M03: loop without break 경고
                if !Self::block_has_break(body) {
                    self.warn(
                        "W005",
                        "Infinite loop: 'loop' has no 'break' statement".to_string(),
                        "Add a 'break;' statement or use 'while cond { }' instead".to_string(),
                    );
                }
                self.check_scoped_block(body, scope, return_ty, true);
            }
            Stmt::While { cond, body } => {
                let cond_ty = self.infer_expr(cond, scope);
                if let Some(ct) = &cond_ty {
                    if *ct != Ty::Bool {
                        self.err(
                            "E006",
                            format!("While condition must be bool, got {}", ty_name(ct)),
                            "Use a comparison (e.g. x > 0) or a bool variable".into(),
                        );
                    }
                }
                self.check_scoped_block(body, scope, return_ty, true);
            }
            Stmt::For { from, to, body, .. } => {
                self.infer_expr(from, scope);
                self.infer_expr(to, scope);
                let mut for_scope_log = Vec::new();
                self.collect_decl_with_log(stmt, scope, &mut for_scope_log);
                self.check_scoped_block(body, scope, return_ty, true);
                Self::restore_scope(scope, for_scope_log);
            }
            Stmt::Free(name) => {
                self.err("E033",
                    format!("manual free({}) is not allowed", name),
                    "Vault variables are automatically freed at scope exit — remove this free() call".to_string());
            }
            Stmt::InlineAnchor { body, .. } => {
                let saved = self.in_anchor;
                self.in_anchor = true;
                self.check_scoped_block(body, scope, return_ty, false);
                self.in_anchor = saved;
            }
            Stmt::Match { expr, arms } => {
                let expr_ty = self.infer_expr(expr, scope);
                let mut has_wildcard = false;
                for arm in arms {
                    match &arm.pattern {
                        Pattern::Wildcard => {
                            has_wildcard = true;
                        }
                        Pattern::IntLit(_)
                        | Pattern::FloatLit(_)
                        | Pattern::StringLit(_)
                        | Pattern::Bool(_) => {}
                        Pattern::EnumVariant {
                            enum_name,
                            variant,
                            binding: _,
                        } => {
                            if let Some(variants) = self.enums.get(enum_name) {
                                if !variants.iter().any(|v| v.name == *variant) {
                                    self.err(
                                        "E037",
                                        format!(
                                            "Enum '{}' has no variant '{}'",
                                            enum_name, variant
                                        ),
                                        format!(
                                            "Available variants: {}",
                                            variants
                                                .iter()
                                                .map(|v| v.name.as_str())
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        ),
                                    );
                                }
                            } else {
                                self.err(
                                    "E038",
                                    format!("Unknown enum type '{}'", enum_name),
                                    format!(
                                        "Declare 'enum {} {{ ... }}' before using it",
                                        enum_name
                                    ),
                                );
                            }
                        }
                    }
                    // Add pattern bindings to scope for arm body
                    let mut arm_scope = scope.clone();
                    if let Pattern::EnumVariant {
                        enum_name,
                        variant,
                        binding: Some(bind_name),
                    } = &arm.pattern
                    {
                        // Find the variant's payload type
                        if let Some(variants) = self.enums.get(enum_name) {
                            if let Some(v) = variants.iter().find(|v| v.name == *variant) {
                                if let Some(ref payload_ty) = v.ty {
                                    arm_scope.insert(
                                        bind_name.clone(),
                                        VarInfo {
                                            ty: payload_ty.clone(),
                                            is_vault: false,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    self.check_scoped_block(&arm.body, &mut arm_scope, return_ty, false);
                }
                // 열거형 매치에서 완전성 검사
                if let Some(Ty::Enum(ename)) = &expr_ty {
                    if !has_wildcard {
                        if let Some(variants) = self.enums.get(ename).cloned() {
                            for v in &variants {
                                let covered = arms.iter().any(|a| {
                                    if let Pattern::EnumVariant { variant, .. } = &a.pattern {
                                        variant == &v.name
                                    } else {
                                        false
                                    }
                                });
                                if !covered {
                                    self.warn("W006",
                                        format!("Non-exhaustive match: variant '{}.{}' not covered", ename, v.name),
                                        format!("Add '{}.{} => {{ ... }}' or '_ => {{ ... }}' as a catch-all", ename, v.name));
                                }
                            }
                        }
                    }
                }
            }
            Stmt::ExprStmt(e) => {
                self.infer_expr(e, scope);
            }
            Stmt::ConstDecl { ty, name, value } => {
                // const는 VarDecl과 같이 처리 (불변성은 정적 분석에서)
                let effective_ty = if *ty == Ty::Auto {
                    self.infer_expr(value, scope).unwrap_or(Ty::Int)
                } else {
                    ty.clone()
                };
                scope.insert(
                    name.clone(),
                    VarInfo {
                        ty: effective_ty,
                        is_vault: false,
                    },
                );
                self.infer_expr(value, scope);
            }
        }
    }

    /// H03: 정수 리터럴이 대상 타입 범위를 초과하는지 검사
    fn check_int_range(&mut self, ty: &Ty, value: &Expr) {
        if let Some((lo, hi)) = int_range(ty) {
            if let Some(val) = const_int_value(value) {
                if val < lo || val > hi {
                    self.warn(
                        "W002",
                        format!(
                            "Integer literal {} overflows type {} (range {}..{})",
                            val,
                            ty_name(ty),
                            lo,
                            hi
                        ),
                        format!("Use a larger type or a value within {}..{}", lo, hi),
                    );
                }
            }
        }
    }

    /// H06: 순환 참조 구조체 감지
    fn check_circular_struct(&self, name: &str, visiting: &mut HashSet<String>) -> bool {
        if !visiting.insert(name.to_string()) {
            return true; // cycle detected
        }
        if let Some(fields) = self.structs.get(name) {
            for field in fields {
                if let Ty::Struct(ref sname) = field.ty {
                    if self.check_circular_struct(sname, visiting) {
                        return true;
                    }
                }
            }
        }
        visiting.remove(name);
        false
    }

    fn infer_expr(&mut self, expr: &Expr, scope: &HashMap<String, VarInfo>) -> Option<Ty> {
        match expr {
            Expr::IntLit(_) => Some(Ty::Int),
            Expr::FloatLit(_) => Some(Ty::Float),
            Expr::StringLit(_) => Some(Ty::String),
            Expr::Bool(_) => Some(Ty::Bool),

            Expr::Ident(name) => {
                self.used_vars.insert(name.clone());
                if let Some(info) = scope.get(name) {
                    Some(info.ty.clone())
                } else {
                    self.err_undeclared_var(name, scope);
                    None
                }
            }

            Expr::UnaryOp { op, expr } => {
                let inner = self.infer_expr(expr, scope);
                match op {
                    UnaryOpKind::Neg => {
                        if let Some(ref t) = inner {
                            if !is_numeric_ty(t) {
                                self.err(
                                    "E007",
                                    format!("Cannot negate type {}", ty_name(t)),
                                    "Negation (-) only works on numeric types".into(),
                                );
                            }
                        }
                        inner
                    }
                    UnaryOpKind::Not => {
                        if let Some(ref t) = inner {
                            if *t != Ty::Bool {
                                self.err(
                                    "E007",
                                    format!("Cannot apply '!' to type {}", ty_name(t)),
                                    "Logical not (!) only works on bool values".into(),
                                );
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
                    BinOpKind::Add
                    | BinOpKind::Sub
                    | BinOpKind::Mul
                    | BinOpKind::Div
                    | BinOpKind::Mod => {
                        if matches!(op, BinOpKind::Add)
                            && matches!((&lt, &rt), (Some(Ty::String), _) | (_, Some(Ty::String)))
                        {
                            // string + non-string -> auto-concat (handled in codegen)
                            return Some(Ty::String);
                        }
                        // string -, *, /, % are invalid
                        if let (Some(ref l), Some(ref _r)) = (&lt, &rt) {
                            if *l == Ty::String {
                                self.err(
                                    "E008",
                                    format!(
                                        "Cannot use '{}' on string type",
                                        match op {
                                            BinOpKind::Sub => "-",
                                            BinOpKind::Mul => "*",
                                            BinOpKind::Div => "/",
                                            BinOpKind::Mod => "%",
                                            _ => "?",
                                        }
                                    ),
                                    "Only '+' is allowed for string concatenation".into(),
                                );
                                return lt;
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
                    BinOpKind::Lt
                    | BinOpKind::Gt
                    | BinOpKind::Le
                    | BinOpKind::Ge
                    | BinOpKind::Eq
                    | BinOpKind::Neq => {
                        if let (Some(ref l), Some(ref r)) = (&lt, &rt) {
                            if !types_compatible(l, r) && !types_compatible(r, l) {
                                self.err(
                                    "E009",
                                    format!(
                                        "Comparison type mismatch \u{2014} {} vs {}",
                                        ty_name(l),
                                        ty_name(r)
                                    ),
                                    "Both sides of a comparison must be the same type".into(),
                                );
                            }
                        }
                        Some(Ty::Bool)
                    }
                    BinOpKind::And | BinOpKind::Or => {
                        if let Some(ref l) = lt {
                            if *l != Ty::Bool {
                                self.err(
                                    "E010",
                                    format!("Logical operator requires bool, got {}", ty_name(l)),
                                    "Use a comparison (e.g. x > 0) to produce a bool".into(),
                                );
                            }
                        }
                        if let Some(ref r) = rt {
                            if *r != Ty::Bool {
                                self.err(
                                    "E010",
                                    format!("Logical operator requires bool, got {}", ty_name(r)),
                                    "Use a comparison (e.g. x > 0) to produce a bool".into(),
                                );
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
                        self.err(
                            "E011",
                            format!("'len' expects 1 argument, got {}", args.len()),
                            "Usage: len(array)".into(),
                        );
                    } else {
                        let arg_ty = self.infer_expr(&args[0], scope);
                        if let Some(ref at) = arg_ty {
                            if !matches!(at, Ty::Array(_)) {
                                self.err(
                                    "E015",
                                    format!("'len' expects an array, got {}", ty_name(at)),
                                    "Pass an array variable to len()".into(),
                                );
                            }
                        }
                    }
                    return Some(Ty::Int);
                }

                if let Some(sig) = self.functions.get(name).cloned() {
                    if args.len() != sig.params.len() {
                        self.err(
                            "E011",
                            format!(
                                "'{}' expects {} argument(s), got {}",
                                name,
                                sig.params.len(),
                                args.len()
                            ),
                            format!(
                                "Signature: {}({})",
                                name,
                                sig.params
                                    .iter()
                                    .map(ty_name)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = self.infer_expr(arg, scope);
                        if let (Some(ref at), Some(expected)) = (&arg_ty, sig.params.get(i)) {
                            if !types_compatible(expected, at) {
                                self.err(
                                    "E012",
                                    format!(
                                        "Arg {} of '{}' \u{2014} expected {}, got {}",
                                        i + 1,
                                        name,
                                        ty_name(expected),
                                        ty_name(at)
                                    ),
                                    format!(
                                        "Pass a {} value as argument {}",
                                        ty_name(expected),
                                        i + 1
                                    ),
                                );
                            }
                        }
                    }
                    sig.return_ty
                } else if scope.contains_key(name) {
                    // 스코프에 있는 변수 → 클로저/함수 포인터 호출
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    // 반환 타입 추론: Ty::Fn의 ret 타입이거나 Int로 fallback
                    if let Some(var_info) = scope.get(name) {
                        if let Ty::Fn(_, ret_ty) = &var_info.ty {
                            ret_ty.as_ref().map(|t| *t.clone())
                        } else {
                            Some(Ty::Int)
                        }
                    } else {
                        Some(Ty::Int)
                    }
                } else {
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    self.err_undeclared_fn(name);
                    None
                }
            }

            Expr::MethodCall { base, method, args } => {
                // mod.func() 호출 처리
                if let Expr::Ident(base_name) = base.as_ref() {
                    if self.module_names.contains(base_name.as_str()) {
                        let qualified = format!("{}_{}", base_name, method);
                        if let Some(sig) = self.functions.get(&qualified).cloned() {
                            if args.len() != sig.params.len() {
                                self.err(
                                    "E011",
                                    format!(
                                        "'{}' expects {} argument(s), got {}",
                                        qualified,
                                        sig.params.len(),
                                        args.len()
                                    ),
                                    format!("Call as {}.{}(...)", base_name, method),
                                );
                            }
                            for (i, arg) in args.iter().enumerate() {
                                let arg_ty = self.infer_expr(arg, scope);
                                if let (Some(ref at), Some(expected)) = (&arg_ty, sig.params.get(i))
                                {
                                    if !types_compatible(expected, at) {
                                        self.err(
                                            "E012",
                                            format!(
                                                "Arg {} of '{}' — expected {}, got {}",
                                                i + 1,
                                                qualified,
                                                ty_name(expected),
                                                ty_name(at)
                                            ),
                                            format!(
                                                "Pass a {} value as argument {}",
                                                ty_name(expected),
                                                i + 1
                                            ),
                                        );
                                    }
                                }
                            }
                            return sig.return_ty;
                        } else {
                            for arg in args {
                                self.infer_expr(arg, scope);
                            }
                            self.err(
                                "E013",
                                format!("Undeclared function '{}.{}'", base_name, method),
                                format!(
                                    "Define it with mod {} {{ fn {}(...) {{ ... }} }}",
                                    base_name, method
                                ),
                            );
                            return None;
                        }
                    }
                }
                let base_ty = self.infer_expr(base, scope);
                let sname = if let Some(Ty::Struct(n)) = base_ty.clone() {
                    n
                } else {
                    self.err(
                        "E028",
                        format!("Cannot call method '{}' on non-struct value", method),
                        "Method call requires a struct value (e.g. user.method()) or a module (e.g. mod.func())".to_string(),
                    );
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    return None;
                };

                let fn_name = format!("{}.{}", sname, method);
                if let Some(sig) = self.functions.get(&fn_name).cloned() {
                    // method는 self 파라미터(자동 삽입) + 사용자 인자들
                    let expected_user_args = sig.params.len().saturating_sub(1);
                    if args.len() != expected_user_args {
                        self.err(
                            "E011",
                            format!(
                                "'{}' expects {} argument(s), got {}",
                                fn_name,
                                expected_user_args,
                                args.len()
                            ),
                            format!("Method signature: {}(...)", fn_name),
                        );
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = self.infer_expr(arg, scope);
                        if let (Some(ref at), Some(expected)) = (&arg_ty, sig.params.get(i + 1)) {
                            if !types_compatible(expected, at) {
                                self.err(
                                    "E012",
                                    format!(
                                        "Arg {} of '{}' — expected {}, got {}",
                                        i + 1,
                                        fn_name,
                                        ty_name(expected),
                                        ty_name(at)
                                    ),
                                    format!(
                                        "Pass a {} value as argument {}",
                                        ty_name(expected),
                                        i + 1
                                    ),
                                );
                            }
                        }
                    }
                    sig.return_ty
                } else {
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    self.err(
                        "E013",
                        format!("Undeclared method '{}.{}'", sname, method),
                        format!("Define it with fn {}.{}(...) {{ ... }}", sname, method),
                    );
                    None
                }
            }

            Expr::ArrayLit(elems) => {
                if elems.is_empty() {
                    self.err(
                        "E016",
                        "Empty array literal \u{2014} cannot infer element type".to_string(),
                        "Provide at least one element, e.g. [0]".into(),
                    );
                    return None;
                }
                let first_ty = self.infer_expr(&elems[0], scope);
                for (i, elem) in elems.iter().enumerate().skip(1) {
                    let elem_ty = self.infer_expr(elem, scope);
                    if let (Some(ref ft), Some(ref et)) = (&first_ty, &elem_ty) {
                        if ft != et {
                            self.err(
                                "E002",
                                format!(
                                    "Array element {} has type {}, expected {}",
                                    i,
                                    ty_name(et),
                                    ty_name(ft)
                                ),
                                "All array elements must be the same type".into(),
                            );
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
                        self.err(
                            "E014",
                            format!("Array index must be int, got {}", ty_name(it)),
                            "Use an integer value for array indexing".into(),
                        );
                    }
                }
                if let Some(Ty::Array(inner)) = arr_ty {
                    Some(*inner)
                } else {
                    if let Some(ref t) = arr_ty {
                        self.err(
                            "E015",
                            format!("Cannot index into non-array type {}", ty_name(t)),
                            "Only array types support indexing".into(),
                        );
                    }
                    None
                }
            }

            Expr::Cast {
                expr,
                ty: target_ty,
            } => {
                let src_ty = self.infer_expr(expr, scope);
                if let Some(ref st) = src_ty {
                    let valid = match (st, target_ty) {
                        // numeric ??numeric
                        (s, t) if is_numeric_ty(s) && is_numeric_ty(t) => true,
                        // bool ??int
                        (Ty::Bool, t) if is_integer_ty(t) => true,
                        // int ??bool
                        (s, Ty::Bool) if is_integer_ty(s) => true,
                        _ => false,
                    };
                    if !valid {
                        self.err(
                            "E021",
                            format!("Cannot cast {} to {}", ty_name(st), ty_name(target_ty)),
                            "Only numeric type conversions are allowed with 'as'".into(),
                        );
                    }
                }
                Some(target_ty.clone())
            }
            Expr::StructInit { name, fields } => {
                if let Some(def_fields) = self.structs.get(name).cloned() {
                    for df in &def_fields {
                        if !fields.iter().any(|(fname, _)| fname == &df.name) {
                            self.err(
                                "E029",
                                format!("Missing field '{}' in struct init '{}'", df.name, name),
                                format!("Provide '{}: ...' in {} {{ ... }}", df.name, name),
                            );
                        }
                    }
                    for (fname, fexpr) in fields {
                        if let Some(df) = def_fields.iter().find(|f| f.name == *fname) {
                            let got = self.infer_expr(fexpr, scope);
                            if let Some(gt) = got {
                                if !types_compatible(&df.ty, &gt) {
                                    self.err(
                                        "E002",
                                        format!(
                                            "Type mismatch for field '{}.{}' — expected {}, got {}",
                                            name,
                                            fname,
                                            ty_name(&df.ty),
                                            ty_name(&gt)
                                        ),
                                        format!(
                                            "Field '{}.{}' type is {}",
                                            name,
                                            fname,
                                            ty_name(&df.ty)
                                        ),
                                    );
                                }
                            }
                        } else {
                            self.err(
                                "E027",
                                format!("Struct '{}' has no field '{}'", name, fname),
                                "Check the struct declaration for available fields".to_string(),
                            );
                            self.infer_expr(fexpr, scope);
                        }
                    }
                } else {
                    self.err(
                        "E026",
                        format!("Unknown struct type '{}'", name),
                        format!("Declare 'struct {} {{ ... }}' before using it", name),
                    );
                    for (_, fexpr) in fields {
                        self.infer_expr(fexpr, scope);
                    }
                }
                Some(Ty::Struct(name.clone()))
            }
            Expr::FieldAccess { base, field } => {
                let bt = self.infer_expr(base, scope);
                if let Some(Ty::Struct(sname)) = bt {
                    if let Some(fields) = self.structs.get(&sname) {
                        if let Some(sf) = fields.iter().find(|f| f.name == *field) {
                            Some(sf.ty.clone())
                        } else {
                            self.err(
                                "E027",
                                format!("Struct '{}' has no field '{}'", sname, field),
                                "Check the struct declaration for available fields".to_string(),
                            );
                            None
                        }
                    } else {
                        self.err(
                            "E026",
                            format!("Unknown struct type '{}'", sname),
                            format!("Declare 'struct {} {{ ... }}' before using it", sname),
                        );
                        None
                    }
                } else {
                    if let Some(t) = bt {
                        self.err(
                            "E028",
                            format!(
                                "Cannot access field '{}' on non-struct type {}",
                                field,
                                ty_name(&t)
                            ),
                            "Field access requires a struct value".to_string(),
                        );
                    }
                    None
                }
            }
            Expr::EnumVariant {
                enum_name,
                variant,
                value,
            } => {
                if let Some(variants) = self.enums.get(enum_name).cloned() {
                    if let Some(v) = variants.iter().find(|v| v.name == *variant) {
                        match (&v.ty, value) {
                            (Some(expected_ty), Some(val_expr)) => {
                                let val_ty = self.infer_expr(val_expr, scope);
                                if let Some(vt) = &val_ty {
                                    if !types_compatible(expected_ty, vt) {
                                        self.err(
                                            "E002",
                                            format!(
                                                "Enum variant '{}.{}' expects {}, got {}",
                                                enum_name,
                                                variant,
                                                ty_name(expected_ty),
                                                ty_name(vt)
                                            ),
                                            format!(
                                                "Pass a {} value to {}.{}",
                                                ty_name(expected_ty),
                                                enum_name,
                                                variant
                                            ),
                                        );
                                    }
                                }
                            }
                            (None, Some(val_expr)) => {
                                self.infer_expr(val_expr, scope);
                                self.err(
                                    "E039",
                                    format!(
                                        "Enum variant '{}.{}' takes no payload",
                                        enum_name, variant
                                    ),
                                    format!("Use '{}.{}' without parentheses", enum_name, variant),
                                );
                            }
                            (Some(expected_ty), None) => {
                                self.err(
                                    "E039",
                                    format!(
                                        "Enum variant '{}.{}' requires a {} payload",
                                        enum_name,
                                        variant,
                                        ty_name(expected_ty)
                                    ),
                                    format!("Use '{}.{}(value)'", enum_name, variant),
                                );
                            }
                            (None, None) => {}
                        }
                    } else {
                        self.err(
                            "E037",
                            format!("Enum '{}' has no variant '{}'", enum_name, variant),
                            format!(
                                "Available variants: {}",
                                variants
                                    .iter()
                                    .map(|v| v.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                } else {
                    self.err(
                        "E038",
                        format!("Unknown enum type '{}'", enum_name),
                        format!("Declare 'enum {} {{ ... }}' before using it", enum_name),
                    );
                }
                Some(Ty::Enum(enum_name.clone()))
            }
            Expr::Closure { params, body } => {
                // 클로저 본체 분석 (파라미터 추가)
                let mut closure_scope = scope.clone();
                let param_tys: Vec<Ty> = params
                    .iter()
                    .map(|(_, opt_ty)| opt_ty.clone().unwrap_or(Ty::Auto))
                    .collect();
                for (pname, opt_ty) in params {
                    let ty = opt_ty.clone().unwrap_or(Ty::Auto);
                    closure_scope.insert(
                        pname.clone(),
                        VarInfo {
                            ty,
                            is_vault: false,
                        },
                    );
                }
                let fn_name = "<closure>";
                let ret_ty: Option<Ty> = None;
                self.check_stmts(body, &mut closure_scope, ret_ty.as_ref(), fn_name);
                Some(Ty::Fn(param_tys, None))
            }
            Expr::FStringLit(parts) => {
                for part in parts {
                    if let crate::ast::FStringPart::Expr(e) = part {
                        self.infer_expr(e, scope);
                    }
                }
                Some(Ty::String)
            }
        }
    }
}

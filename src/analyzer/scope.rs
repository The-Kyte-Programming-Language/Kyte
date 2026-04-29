use std::collections::HashMap;

use crate::analyzer::shared::VarInfo;
use crate::analyzer::Analyzer;
use crate::ast::*;

impl Analyzer {
    pub(super) fn collect_decl_with_log(
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

    pub(super) fn restore_scope(
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

    pub(super) fn check_scoped_block(
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

    pub(super) fn check_stmts(
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

    pub(super) fn collect_decl(&mut self, stmt: &Stmt, scope: &mut HashMap<String, VarInfo>) {
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

    pub(super) fn check_anchor(
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
}

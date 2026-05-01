use std::collections::HashMap;

use crate::analyzer::shared::{ty_name, types_compatible, VarInfo};
use crate::analyzer::Analyzer;
use crate::ast::*;

impl Analyzer {
    pub(super) fn check_stmt_scoped(
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
            Stmt::Exit | Stmt::Break | Stmt::Continue => {}
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
                // E050: Vault 할당은 루프 내에서 허용되지 않음 (메모리 누수 위험)
                self.check_vault_in_loop(body, "loop");
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
                // E050: Vault 할당은 루프 내에서 허용되지 않음
                self.check_vault_in_loop(body, "while");
                self.check_scoped_block(body, scope, return_ty, true);
            }
            Stmt::For { from, to, body, .. } => {
                self.infer_expr(from, scope);
                self.infer_expr(to, scope);
                // E050: Vault 할당은 루프 내에서 허용되지 않음
                self.check_vault_in_loop(body, "for");
                let mut for_scope_log = Vec::new();
                self.collect_decl_with_log(stmt, scope, &mut for_scope_log);
                self.check_scoped_block(body, scope, return_ty, true);
                Self::restore_scope(scope, for_scope_log);
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

    // ── Vault-in-loop detection ────────────────────────────────────────────────

    /// E050: Vault 할당이 루프 내에 있으면 컴파일 오류.
    ///
    /// 루프마다 새로운 힙 메모리를 할당하지만, 루프 종료 시 자동 해제 스코프가
    /// 올바르게 발생하지 않을 수 있어 메모리 누수가 발생합니다.
    /// 루프 외부에서 Vault를 선언하거나, 루프 내에서 명시적으로 free()를 사용하세요.
    pub(super) fn check_vault_in_loop(&mut self, body: &[(Stmt, Span)], loop_kind: &str) {
        for (stmt, _span) in body {
            if let Stmt::VaultDecl { name, .. } = stmt {
                self.err(
                    "E050",
                    format!(
                        "Vault '{}' allocated inside '{}' loop — potential memory accumulation",
                        name, loop_kind
                    ),
                    format!(
                        "Move 'Vault {}' outside the loop, or restructure to avoid repeated heap allocation",
                        name
                    ),
                );
            }
            // Recurse into nested non-loop blocks (if/else bodies)
            // but NOT into inner loops (they will self-check) or anchors
            // Do NOT recurse into nested loops (they check themselves)
            // or InlineAnchor (separate supervision domain)
            if let Stmt::If {
                then_body,
                else_body,
                ..
            } = stmt
            {
                self.check_vault_in_loop(then_body, loop_kind);
                if let Some(eb) = else_body {
                    self.check_vault_in_loop(eb, loop_kind);
                }
            }
        }
    }
}

use inkwell::values::BasicValueEnum;

use super::Codegen;
use crate::ast::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn store_var(&self, name: &str, val: BasicValueEnum<'ctx>) {
        let ptr = self.variables[name];
        if self.vault_vars.contains(name) {
            let heap_ptr = self
                .builder
                .build_load(self.ptr_type(), ptr, "vptr")
                .unwrap()
                .into_pointer_value();
            self.builder.build_store(heap_ptr, val).unwrap();
        } else {
            self.builder.build_store(ptr, val).unwrap();
        }
    }

    pub(super) fn load_var(&self, name: &str, ty: &Ty) -> BasicValueEnum<'ctx> {
        if let Some(&ptr) = self.variables.get(name) {
            if self.vault_vars.contains(name) {
                let heap_ptr = self
                    .builder
                    .build_load(self.ptr_type(), ptr, "vptr")
                    .unwrap()
                    .into_pointer_value();
                let llvm_ty = self.ty_to_basic(ty);
                return self.builder.build_load(llvm_ty, heap_ptr, name).unwrap();
            }
            let llvm_ty = self.ty_to_basic(ty);
            return self.builder.build_load(llvm_ty, ptr, name).unwrap();
        }
        // 전역 상수 로드
        if let Some(global) = self.module.get_global(name) {
            let llvm_ty = self.ty_to_basic(ty);
            return self
                .builder
                .build_load(llvm_ty, global.as_pointer_value(), name)
                .unwrap();
        }
        // fallback: 0
        self.i64_type().const_int(0, false).into()
    }

    pub(super) fn free_vault_var(&mut self, name: &str) {
        if !self.vault_vars.contains(name) {
            return;
        }
        if self.freed_vault_vars.contains(name) {
            return;
        }
        if let Some(ptr) = self.variables.get(name).copied() {
            let heap_ptr = self
                .builder
                .build_load(self.ptr_type(), ptr, "vptr")
                .unwrap()
                .into_pointer_value();
            let free_fn = self.module.get_function("free").unwrap();
            self.builder
                .build_call(free_fn, &[heap_ptr.into()], "")
                .unwrap();
            self.builder
                .build_store(ptr, self.ptr_type().const_null())
                .unwrap();
            self.freed_vault_vars.insert(name.to_string());
            // 런타임 vault 카운터 감소
            if let Some(counter) = self.vault_live_count {
                let cur = self
                    .builder
                    .build_load(self.i64_type(), counter, "vlc")
                    .unwrap()
                    .into_int_value();
                let next = self
                    .builder
                    .build_int_sub(cur, self.i64_type().const_int(1, false), "vlc_dec")
                    .unwrap();
                self.builder.build_store(counter, next).unwrap();
            }
        }
    }

    pub(super) fn register_vault_in_current_scope(&mut self, name: &str) {
        if let Some(scope) = self.vault_scope_stack.last_mut() {
            scope.push(name.to_string());
        }
    }

    pub(super) fn cleanup_current_scope(&mut self) {
        if let Some(names) = self.vault_scope_stack.pop() {
            for name in names.into_iter().rev() {
                self.free_vault_var(&name);
            }
        }
        // M05: 임시 문자열 버퍼 해제
        if let Some(ptrs) = self.string_temp_stack.pop() {
            if let Some(free_fn) = self.module.get_function("free") {
                for ptr in ptrs {
                    self.builder.build_call(free_fn, &[ptr.into()], "").unwrap();
                }
            }
        }
    }

    pub(super) fn cleanup_to_depth(&mut self, target_depth: usize) {
        while self.vault_scope_stack.len() > target_depth {
            self.cleanup_current_scope();
        }
    }

    pub(super) fn guess_var_ty(&self, name: &str, params: &[Param]) -> Ty {
        if let Some(ty) = self.var_types.get(name) {
            return ty.clone();
        }
        for p in params {
            if p.name == name {
                return p.ty.clone();
            }
        }
        Ty::Int
    }

    pub(super) fn guess_expr_ty(&self, expr: &Expr, params: &[Param]) -> Ty {
        match expr {
            Expr::IntLit(_) => Ty::Int,
            Expr::FloatLit(_) => Ty::Float,
            Expr::StringLit(_) => Ty::String,
            Expr::Bool(_) => Ty::Bool,
            Expr::Ident(name, _) => self.guess_var_ty(name, params),
            Expr::UnaryOp { op, expr } => match op {
                UnaryOpKind::Not => Ty::Bool,
                UnaryOpKind::Neg => self.guess_expr_ty(expr, params),
            },
            Expr::BinOp { left, op, .. } => match op {
                BinOpKind::Lt
                | BinOpKind::Gt
                | BinOpKind::Le
                | BinOpKind::Ge
                | BinOpKind::Eq
                | BinOpKind::Neq
                | BinOpKind::And
                | BinOpKind::Or => Ty::Bool,
                _ => self.guess_expr_ty(left, params),
            },
            Expr::Call { name, .. } => self
                .fn_return_tys
                .get(name)
                .and_then(|opt| opt.as_ref())
                .cloned()
                .unwrap_or(Ty::Int),
            Expr::ArrayLit(elems) => {
                if elems.is_empty() {
                    Ty::Array(Box::new(Ty::Int))
                } else {
                    Ty::Array(Box::new(self.guess_expr_ty(&elems[0], params)))
                }
            }
            Expr::Index { array, .. } => match self.guess_expr_ty(array, params) {
                Ty::Array(inner) => *inner,
                _ => Ty::Int,
            },
            Expr::Cast { ty, .. } => ty.clone(),
            Expr::StructInit { name, .. } => Ty::Struct(name.clone()),
            Expr::FieldAccess { base, field } => {
                if let Ty::Struct(sname) = self.guess_expr_ty(base, params) {
                    self.struct_field_info(&sname, field)
                        .map(|(_, t)| t)
                        .unwrap_or(Ty::Int)
                } else {
                    Ty::Int
                }
            }
            Expr::MethodCall { base, method, .. } => {
                if let Ty::Struct(sname) = self.guess_expr_ty(base, params) {
                    let fn_name = format!("{}.{}", sname, method);
                    self.fn_return_tys
                        .get(&fn_name)
                        .and_then(|opt| opt.as_ref())
                        .cloned()
                        .unwrap_or(Ty::Int)
                } else {
                    Ty::Int
                }
            }
            Expr::EnumVariant { enum_name, .. } => Ty::Enum(enum_name.clone()),
            Expr::Closure { .. } => Ty::Fn(vec![], None),
            Expr::FStringLit(_) => Ty::String,
        }
    }
}

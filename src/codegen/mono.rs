use std::collections::HashMap;

use inkwell::types::{BasicMetadataTypeEnum, BasicType};

use super::Codegen;
use crate::ast::*;

// ── Type mangling ────────────────────────────────────────────────────────────

fn mangle_ty(ty: &Ty) -> String {
    match ty {
        Ty::Int => "int".to_string(),
        Ty::I8 => "i8".to_string(),
        Ty::I16 => "i16".to_string(),
        Ty::I32 => "i32".to_string(),
        Ty::I64 => "i64".to_string(),
        Ty::U8 => "u8".to_string(),
        Ty::U16 => "u16".to_string(),
        Ty::U32 => "u32".to_string(),
        Ty::U64 => "u64".to_string(),
        Ty::Float => "float".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::String => "str".to_string(),
        Ty::Array(inner) => format!("arr{}", mangle_ty(inner)),
        Ty::Struct(name) => format!("s{}", name),
        Ty::Enum(name) => format!("e{}", name),
        Ty::TypeParam(name) => format!("tp{}", name),
        Ty::Auto | Ty::Fn(_, _) => "auto".to_string(),
    }
}

/// Produce the LLVM-level function name for a generic specialization.
pub(super) fn mangle_name(fn_name: &str, type_args: &[Ty]) -> String {
    if type_args.is_empty() {
        return fn_name.to_string();
    }
    let suffix: Vec<String> = type_args.iter().map(mangle_ty).collect();
    format!("{}__{}", fn_name, suffix.join("_"))
}

// ── Type substitution ────────────────────────────────────────────────────────

pub(super) fn subst_ty(ty: &Ty, type_params: &[String], type_args: &[Ty]) -> Ty {
    match ty {
        Ty::TypeParam(name) => {
            if let Some(idx) = type_params.iter().position(|p| p == name) {
                type_args.get(idx).cloned().unwrap_or(Ty::Auto)
            } else {
                ty.clone()
            }
        }
        Ty::Array(inner) => Ty::Array(Box::new(subst_ty(inner, type_params, type_args))),
        Ty::Fn(param_tys, ret) => Ty::Fn(
            param_tys
                .iter()
                .map(|t| subst_ty(t, type_params, type_args))
                .collect(),
            ret.as_ref()
                .map(|t| Box::new(subst_ty(t, type_params, type_args))),
        ),
        _ => ty.clone(),
    }
}

// ── Type argument inference ───────────────────────────────────────────────────

/// Infer concrete type arguments from the types of call-site arguments.
/// Matches each function parameter's TypeParam to the corresponding arg type.
pub(super) fn infer_type_args(
    type_params: &[String],
    fn_params: &[Param],
    arg_tys: &[Ty],
) -> Vec<Ty> {
    let mut map: HashMap<String, Ty> = HashMap::new();
    for (param, arg_ty) in fn_params.iter().zip(arg_tys.iter()) {
        if let Ty::TypeParam(name) = &param.ty {
            map.entry(name.clone()).or_insert_with(|| arg_ty.clone());
        }
    }
    type_params
        .iter()
        .map(|tp| map.get(tp).cloned().unwrap_or(Ty::Auto))
        .collect()
}

// ── Specialization emission ───────────────────────────────────────────────────

impl<'ctx> Codegen<'ctx> {
    /// Infer type arguments from arg types and emit a specialization if needed.
    /// Returns the mangled function name ready to call.
    pub(super) fn emit_specialization_for_args(
        &mut self,
        fn_name: &str,
        arg_tys: &[Ty],
    ) -> String {
        let gdef = match self.generic_defs.get(fn_name).cloned() {
            Some(d) => d,
            None => return fn_name.to_string(),
        };
        let (type_params, fn_params) = match &gdef {
            TopLevel::Function {
                type_params,
                params,
                ..
            } => (type_params.clone(), params.clone()),
            _ => return fn_name.to_string(),
        };
        let type_args = infer_type_args(&type_params, &fn_params, arg_tys);
        self.emit_specialization(fn_name, &type_args)
    }

    /// Emit a concrete specialization of a generic function for the given type args.
    /// Returns the mangled LLVM function name.
    pub(super) fn emit_specialization(&mut self, fn_name: &str, type_args: &[Ty]) -> String {
        let mangled = mangle_name(fn_name, type_args);

        // Already emitted — deduplicate
        if self.functions.contains_key(&mangled) {
            return mangled;
        }

        let gdef = match self.generic_defs.get(fn_name).cloned() {
            Some(d) => d,
            None => return fn_name.to_string(),
        };

        let (type_params, params, return_ty, body) = match gdef {
            TopLevel::Function {
                type_params,
                params,
                return_ty,
                body,
                ..
            } => (type_params, params, return_ty, body),
            _ => return fn_name.to_string(),
        };

        // Substitute TypeParam in the function signature
        let concrete_params: Vec<Param> = params
            .iter()
            .map(|p| Param {
                name: p.name.clone(),
                ty: subst_ty(&p.ty, &type_params, type_args),
            })
            .collect();
        let concrete_return_ty: Option<Ty> =
            return_ty.as_ref().map(|t| subst_ty(t, &type_params, type_args));

        // Set up type substitution context for this specialization
        let old_subst = std::mem::take(&mut self.type_subst);
        for (tp, ta) in type_params.iter().zip(type_args.iter()) {
            self.type_subst.insert(tp.clone(), ta.clone());
        }

        // Emit LLVM prototype (must happen before compiling body for recursion)
        let param_types: Vec<BasicMetadataTypeEnum> = concrete_params
            .iter()
            .map(|p| self.ty_to_llvm(&p.ty))
            .collect();
        let fn_type = match &concrete_return_ty {
            Some(ty) => self.ty_to_basic(ty).fn_type(&param_types, false),
            None => self.context.void_type().fn_type(&param_types, false),
        };
        let func = self.module.add_function(&mangled, fn_type, None);
        self.functions.insert(mangled.clone(), func);
        self.fn_return_tys
            .insert(mangled.clone(), concrete_return_ty.clone());

        // ── Save caller compilation state ────────────────────────────────────
        let saved_fn = self.current_fn;
        let saved_bb = self.builder.get_insert_block();
        let saved_vars = std::mem::take(&mut self.variables);
        let saved_var_types = std::mem::take(&mut self.var_types);
        let saved_vault_scope = std::mem::take(&mut self.vault_scope_stack);
        let saved_freed = std::mem::take(&mut self.freed_vault_vars);
        let saved_break_depth = self.break_cleanup_depth.take();
        let saved_kill_stack = std::mem::take(&mut self.kill_cleanup_depth_stack);
        let saved_anchor_restart = std::mem::take(&mut self.anchor_restart_bb_stack);
        let saved_vault_live = self.vault_live_count.take();
        let saved_break_bb = self.break_bb.take();
        let saved_continue_bb = self.continue_bb.take();
        let saved_recovery = std::mem::take(&mut self.recovery_stack);
        let saved_yield_slot = std::mem::take(&mut self.yield_slot);
        let saved_yield_merge = std::mem::take(&mut self.yield_merge_bb);
        let saved_kill_count = std::mem::take(&mut self.kill_count_slot);
        let saved_catch_bb = std::mem::take(&mut self.catch_bb_stack);
        let saved_catch_msg = std::mem::take(&mut self.catch_msg_slot_stack);
        let saved_anchor_name = std::mem::take(&mut self.anchor_name_stack);
        let saved_string_temp = std::mem::take(&mut self.string_temp_stack);

        // ── Compile specialized body ─────────────────────────────────────────
        self.current_fn = Some(func);
        let entry = self.context.append_basic_block(func, "entry");
        self.builder.position_at_end(entry);

        let vlc = self.build_alloca("vault_live_count", &Ty::I64);
        self.builder
            .build_store(vlc, self.i64_type().const_int(0, false))
            .unwrap();
        self.vault_live_count = Some(vlc);

        for (i, p) in concrete_params.iter().enumerate() {
            let alloca = self.build_alloca(&p.name, &p.ty);
            self.builder
                .build_store(alloca, func.get_nth_param(i as u32).unwrap())
                .unwrap();
            self.variables.insert(p.name.clone(), alloca);
            self.var_types.insert(p.name.clone(), p.ty.clone());
        }

        self.compile_stmts(&body, &concrete_params);

        if self.no_terminator() {
            match &concrete_return_ty {
                None => {
                    self.builder.build_return(None).unwrap();
                }
                Some(Ty::Float) => {
                    self.builder
                        .build_return(Some(&self.f64_type().const_float(0.0)))
                        .unwrap();
                }
                Some(Ty::String) | Some(Ty::Array(_)) => {
                    self.builder
                        .build_return(Some(&self.ptr_type().const_null()))
                        .unwrap();
                }
                Some(ty) => {
                    let int_ty = self.ty_to_int_type(ty);
                    self.builder
                        .build_return(Some(&int_ty.const_int(0, false)))
                        .unwrap();
                }
            }
        }

        // ── Restore caller compilation state ─────────────────────────────────
        self.current_fn = saved_fn;
        if let Some(bb) = saved_bb {
            self.builder.position_at_end(bb);
        }
        self.variables = saved_vars;
        self.var_types = saved_var_types;
        self.vault_scope_stack = saved_vault_scope;
        self.freed_vault_vars = saved_freed;
        self.break_cleanup_depth = saved_break_depth;
        self.kill_cleanup_depth_stack = saved_kill_stack;
        self.anchor_restart_bb_stack = saved_anchor_restart;
        self.vault_live_count = saved_vault_live;
        self.break_bb = saved_break_bb;
        self.continue_bb = saved_continue_bb;
        self.recovery_stack = saved_recovery;
        self.yield_slot = saved_yield_slot;
        self.yield_merge_bb = saved_yield_merge;
        self.kill_count_slot = saved_kill_count;
        self.catch_bb_stack = saved_catch_bb;
        self.catch_msg_slot_stack = saved_catch_msg;
        self.anchor_name_stack = saved_anchor_name;
        self.string_temp_stack = saved_string_temp;
        self.type_subst = old_subst;

        mangled
    }
}

use std::collections::HashMap;

use inkwell::types::{BasicMetadataTypeEnum, BasicType};

use super::Codegen;
use crate::ast::*;

// ── Helper: collect Vault variable names declared in a body ──────────────────

fn collect_vault_names(stmts: &[(Stmt, Span)]) -> Vec<String> {
    let mut names = Vec::new();
    for (stmt, _) in stmts {
        collect_vault_names_stmt(stmt, &mut names);
    }
    names
}

fn collect_vault_names_stmt(stmt: &Stmt, out: &mut Vec<String>) {
    match stmt {
        Stmt::VaultDecl { name, .. } => out.push(name.clone()),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for (s, _) in then_body {
                collect_vault_names_stmt(s, out);
            }
            if let Some(eb) = else_body {
                for (s, _) in eb {
                    collect_vault_names_stmt(s, out);
                }
            }
        }
        Stmt::Loop(body) | Stmt::While { body, .. } => {
            for (s, _) in body {
                collect_vault_names_stmt(s, out);
            }
        }
        Stmt::For { body, .. } => {
            for (s, _) in body {
                collect_vault_names_stmt(s, out);
            }
        }
        Stmt::InlineAnchor { body, .. } => {
            // Do NOT recurse into child anchors — they manage their own Vaults
            let _ = body;
        }
        _ => {}
    }
}

// ── Main compile entry ───────────────────────────────────────────────────────

impl<'ctx> Codegen<'ctx> {
    pub fn compile(&mut self, program: &Program) {
        self.declare_printf();
        self.declare_exit_fn();
        self.declare_snprintf();
        self.declare_strlen();
        self.declare_malloc();
        self.declare_free_fn();
        self.declare_strcmp();
        self.declare_kyte_runtime();

        // 0단계: struct/enum 타입 선언/본문 설정
        for (item, _) in &program.items {
            if let TopLevel::Struct { name, fields } = item {
                let st = self.context.opaque_struct_type(name);
                self.struct_types.insert(name.clone(), st);
                self.struct_defs.insert(name.clone(), fields.clone());
            }
            if let TopLevel::Enum { name, variants } = item {
                // enum = { i32 tag, [payload_size x i8] }
                let mut max_payload: u64 = 0;
                let mut tags = HashMap::new();
                for (idx, v) in variants.iter().enumerate() {
                    tags.insert(v.name.clone(), idx as u32);
                    if let Some(ref ty) = v.ty {
                        let sz = self.type_size_bytes(ty);
                        if sz > max_payload {
                            max_payload = sz;
                        }
                    }
                }
                self.enum_variant_tags.insert(name.clone(), tags);
                self.enum_payload_sizes.insert(name.clone(), max_payload);
                self.enum_defs.insert(name.clone(), variants.clone());

                let st = self.context.opaque_struct_type(name);
                let mut body_types: Vec<inkwell::types::BasicTypeEnum<'ctx>> = vec![
                    self.context.i32_type().into(), // tag
                ];
                if max_payload > 0 {
                    body_types.push(self.context.i8_type().array_type(max_payload as u32).into());
                }
                st.set_body(&body_types, false);
                self.enum_types.insert(name.clone(), st);
            }
        }
        for (name, fields) in self.struct_defs.clone() {
            if let Some(st) = self.struct_types.get(&name).copied() {
                let field_types: Vec<_> = fields.iter().map(|f| self.ty_to_basic(&f.ty)).collect();
                st.set_body(&field_types, false);
            }
        }

        // 0.5단계: 최상위 const를 LLVM global variable로 emit
        for (item, _) in &program.items {
            if let TopLevel::ConstDecl { ty, name, value } = item {
                let llvm_ty = self.ty_to_basic(ty);
                let global = self
                    .module
                    .add_global(llvm_ty.as_basic_type_enum(), None, name);
                global.set_constant(true);
                let init_val: inkwell::values::BasicValueEnum = match value {
                    Expr::IntLit(n) => {
                        let it = self.ty_to_int_type(ty);
                        it.const_int(*n as u64, *n < 0).into()
                    }
                    Expr::FloatLit(f) => self.f64_type().const_float(*f).into(),
                    Expr::StringLit(s) => self.global_string_ptr(s, name).into(),
                    Expr::Bool(b) => self.bool_type().const_int(*b as u64, false).into(),
                    _ => llvm_ty.const_zero(),
                };
                global.set_initializer(&init_val);
            }
        }

        // 1단계: 함수 프로토타입 선언
        for (item, _) in &program.items {
            if let TopLevel::Function {
                name,
                params,
                return_ty,
                ..
            } = item
            {
                let param_types: Vec<BasicMetadataTypeEnum> =
                    params.iter().map(|p| self.ty_to_llvm(&p.ty)).collect();

                let fn_type = match return_ty {
                    Some(ty) => {
                        let ret_ty = self.ty_to_basic(ty);
                        ret_ty.fn_type(&param_types, false)
                    }
                    None => self.context.void_type().fn_type(&param_types, false),
                };

                let func = self.module.add_function(name, fn_type, None);
                self.functions.insert(name.clone(), func);
                self.fn_return_tys.insert(name.clone(), return_ty.clone());
            }
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
                        let param_types: Vec<BasicMetadataTypeEnum> =
                            params.iter().map(|p| self.ty_to_llvm(&p.ty)).collect();
                        let fn_type = match return_ty {
                            Some(ty) => self.ty_to_basic(ty).fn_type(&param_types, false),
                            None => self.context.void_type().fn_type(&param_types, false),
                        };
                        let func = self.module.add_function(&qualified, fn_type, None);
                        self.functions.insert(qualified.clone(), func);
                        self.fn_return_tys.insert(qualified, return_ty.clone());
                    }
                }
            }
            if let TopLevel::Module {
                name: modname,
                items: mod_items,
            } = item
            {
                self.module_names.insert(modname.clone());
                for (mod_tl, _) in mod_items {
                    if let TopLevel::Function {
                        name: fn_name,
                        params,
                        return_ty,
                        ..
                    } = mod_tl
                    {
                        let qualified = format!("{}_{}", modname, fn_name);
                        let param_types: Vec<BasicMetadataTypeEnum> =
                            params.iter().map(|p| self.ty_to_llvm(&p.ty)).collect();
                        let fn_type = match return_ty {
                            Some(ty) => self.ty_to_basic(ty).fn_type(&param_types, false),
                            None => self.context.void_type().fn_type(&param_types, false),
                        };
                        let func = self.module.add_function(&qualified, fn_type, None);
                        self.functions.insert(qualified.clone(), func);
                        self.fn_return_tys.insert(qualified, return_ty.clone());
                    }
                }
            }
        }

        // 2단계: 함수 본문 생성
        struct FnToCompile<'a> {
            name: String,
            params: &'a Vec<Param>,
            return_ty: &'a Option<Ty>,
            body: &'a Vec<(Stmt, Span)>,
        }
        let mut fns_to_compile: Vec<FnToCompile> = Vec::new();
        for (item, _) in &program.items {
            if let TopLevel::Function {
                name,
                params,
                return_ty,
                body,
                ..
            } = item
            {
                fns_to_compile.push(FnToCompile {
                    name: name.clone(),
                    params,
                    return_ty,
                    body,
                });
            }
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
                        body,
                        ..
                    } = method_tl
                    {
                        let qname = format!("{}_{}_{}", trait_name, target_ty, mname);
                        fns_to_compile.push(FnToCompile {
                            name: qname,
                            params,
                            return_ty,
                            body,
                        });
                    }
                }
            }
            if let TopLevel::Module {
                name: modname,
                items: mod_items,
            } = item
            {
                for (mod_tl, _) in mod_items {
                    if let TopLevel::Function {
                        name: fn_name,
                        params,
                        return_ty,
                        body,
                        ..
                    } = mod_tl
                    {
                        let qname = format!("{}_{}", modname, fn_name);
                        fns_to_compile.push(FnToCompile {
                            name: qname,
                            params,
                            return_ty,
                            body,
                        });
                    }
                }
            }
        }
        for fn_info in &fns_to_compile {
            let name = &fn_info.name;
            let params = fn_info.params;
            let return_ty = fn_info.return_ty;
            let body = fn_info.body;
            let func = self.functions[name];
            self.current_fn = Some(func);
            self.vault_scope_stack.clear();
            self.freed_vault_vars.clear();
            self.break_cleanup_depth = None;
            self.kill_cleanup_depth_stack.clear();
            self.anchor_restart_bb_stack.clear();
            let entry = self.context.append_basic_block(func, "entry");
            self.builder.position_at_end(entry);

            let vlc = self.build_alloca("vault_live_count", &Ty::I64);
            self.builder
                .build_store(vlc, self.i64_type().const_int(0, false))
                .unwrap();
            self.vault_live_count = Some(vlc);

            let saved_vars = self.variables.clone();
            let saved_types = self.var_types.clone();
            for (i, p) in params.iter().enumerate() {
                let alloca = self.build_alloca(&p.name, &p.ty);
                self.builder
                    .build_store(alloca, func.get_nth_param(i as u32).unwrap())
                    .unwrap();
                self.variables.insert(p.name.clone(), alloca);
                self.var_types.insert(p.name.clone(), p.ty.clone());
            }

            self.compile_stmts(body, params);

            if self.no_terminator() {
                match return_ty {
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

            self.variables = saved_vars;
            self.var_types = saved_types;
        }

        // 3단계: main 앵커 → C main 함수
        for (item, _) in &program.items {
            if let TopLevel::Anchor {
                kind: AnchorKind::Main,
                body,
                children,
                catch_param,
                catch_body,
                ..
            } = item
            {
                let i32_type = self.context.i32_type();
                let main_fn_type = i32_type.fn_type(&[], false);
                let main_fn = self.module.add_function("main", main_fn_type, None);
                self.current_fn = Some(main_fn);
                self.vault_scope_stack.clear();
                self.freed_vault_vars.clear();
                self.break_cleanup_depth = None;
                self.kill_cleanup_depth_stack.clear();
                self.anchor_restart_bb_stack.clear();

                // ── entry block: one-time setup ──────────────────────────────
                let entry = self.context.append_basic_block(main_fn, "entry");
                let main_loop = self.context.append_basic_block(main_fn, "main_loop");
                let main_recover = self.context.append_basic_block(main_fn, "recover_main");
                let main_after = self.context.append_basic_block(main_fn, "after_main");
                let main_catch_bb = if catch_body.is_some() {
                    Some(self.context.append_basic_block(main_fn, "catch_main"))
                } else {
                    None
                };

                self.builder.position_at_end(entry);

                // Install signal handlers once at program start.
                let install_sig = self.module.get_function("kyte_install_signals").unwrap();
                self.builder.build_call(install_sig, &[], "").unwrap();

                // Vault live counter
                let vlc = self.build_alloca("vault_live_count", &Ty::I64);
                self.builder
                    .build_store(vlc, self.i64_type().const_int(0, false))
                    .unwrap();
                self.vault_live_count = Some(vlc);

                // Per-anchor slots (allocated once, persist across restarts)
                let main_yield = self.build_alloca("main_yield", &Ty::I64);
                self.builder
                    .build_store(main_yield, self.i64_type().const_int(0, false))
                    .unwrap();
                let main_kill_count = self.build_alloca("main_kill_count", &Ty::I64);
                self.builder
                    .build_store(main_kill_count, self.i64_type().const_int(0, false))
                    .unwrap();

                // catch message slot (only when catch exists)
                let main_catch_msg_slot = if catch_body.is_some() {
                    let slot = self.build_alloca("main_catch_msg", &Ty::String);
                    let empty = self.global_string_ptr("", "catch_empty");
                    self.builder.build_store(slot, empty).unwrap();
                    Some(slot)
                } else {
                    None
                };

                // Pre-null the Vault pointer allocas for all Vaults in the
                // main body.  This ensures cleanup-before-VaultDecl is safe
                // on restarts.
                let main_vault_names = collect_vault_names(body);

                // We can't allocate these here (we haven't scanned the body
                // yet), but we will emit the null-store at main_loop entry.
                // For now, fall through to main_loop.
                self.builder.build_unconditional_branch(main_loop).unwrap();

                // ── main_loop: anchor restart target ─────────────────────────
                self.builder.position_at_end(main_loop);

                // Call kyte_anchor_enter() — sigsetjmp wrapper.
                // Returns 0 on fresh entry, signal# on signal recovery.
                let enter_fn = self.module.get_function("kyte_anchor_enter").unwrap();
                let jmp_ret = self
                    .builder
                    .build_call(enter_fn, &[], "jmp_ret")
                    .unwrap()
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_int_value();

                // Check: if signal recovery, treat as Kill (go to recover)
                let jmp_is_signal = self
                    .builder
                    .build_int_compare(
                        inkwell::IntPredicate::NE,
                        jmp_ret,
                        self.context.i32_type().const_int(0, false),
                        "signal_caught",
                    )
                    .unwrap();
                let body_start_bb = self.context.append_basic_block(main_fn, "main_body_start");
                self.builder
                    .build_conditional_branch(jmp_is_signal, main_recover, body_start_bb)
                    .unwrap();
                self.builder.position_at_end(body_start_bb);

                // Push recovery context onto supervisor stacks
                self.recovery_stack.push(main_recover);
                self.anchor_restart_bb_stack.push(main_loop);
                self.kill_cleanup_depth_stack
                    .push(self.vault_scope_stack.len());
                self.yield_slot.push(main_yield);
                self.yield_merge_bb.push(main_after);
                self.kill_count_slot.push(main_kill_count);
                self.catch_bb_stack.push(main_catch_bb);
                self.catch_msg_slot_stack.push(main_catch_msg_slot);

                // Null-initialize Vault ptr allocas that haven't been touched
                // yet (important on restarts so cleanup doesn't free garbage).
                for vname in &main_vault_names {
                    if let Some(&ptr) = self.variables.get(vname) {
                        self.builder
                            .build_store(ptr, self.ptr_type().const_null())
                            .unwrap();
                    }
                }

                let main_exp_vaults = self.save_vault_count("main");

                self.compile_stmts(body, &[]);

                // ── child anchors (non-thread) ────────────────────────────────
                for (child, _) in children {
                    if let TopLevel::Anchor {
                        name: child_name,
                        kind: child_kind,
                        body: child_body,
                        children: grandchildren,
                        catch_param: child_catch_param,
                        catch_body: child_catch_body,
                        ..
                    } = child
                    {
                        match child_kind {
                            AnchorKind::Thread => {
                                self.emit_thread_anchor(child_name, child_body, grandchildren);
                            }
                            AnchorKind::Event(ev) => {
                                self.emit_event_anchor(child_name, ev, child_body);
                            }
                            _ => {
                                self.emit_child_anchor(
                                    child_name,
                                    child_body,
                                    grandchildren,
                                    &[],
                                    child_catch_param.as_deref(),
                                    child_catch_body.as_deref(),
                                );
                            }
                        }
                    }
                }

                // Normal completion → after
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(main_after).unwrap();
                }

                // ── recovery block: Kill lands here ───────────────────────────
                self.builder.position_at_end(main_recover);
                // On recovery, call kyte_anchor_exit to pop the sigsetjmp slot
                // (kyte_anchor_enter already decremented depth on longjmp return,
                //  so we do NOT double-pop here — the C side handles it).
                self.emit_recovery_vault_assert(main_loop, main_exp_vaults, "main");

                // Pop supervisor stacks after recovery block is fully emitted.
                self.recovery_stack.pop();
                self.anchor_restart_bb_stack.pop();
                self.kill_cleanup_depth_stack.pop();
                self.yield_slot.pop();
                self.yield_merge_bb.pop();
                self.kill_count_slot.pop();
                self.catch_bb_stack.pop();
                self.catch_msg_slot_stack.pop();

                // ── catch block (cold path) ───────────────────────────────────
                if let (Some(catch_bb), Some(catch_stmts)) = (main_catch_bb, catch_body.as_deref())
                {
                    self.builder.position_at_end(catch_bb);
                    let param_name = catch_param.as_deref().unwrap_or("_reason");
                    if let Some(msg_slot) = main_catch_msg_slot {
                        let msg_val = self
                            .builder
                            .build_load(self.ptr_type(), msg_slot, param_name)
                            .unwrap();
                        let param_alloca = self.build_alloca(param_name, &Ty::String);
                        self.builder.build_store(param_alloca, msg_val).unwrap();
                        self.variables.insert(param_name.to_string(), param_alloca);
                        self.var_types.insert(param_name.to_string(), Ty::String);
                    }
                    let catch_start_depth = self.vault_scope_stack.len();
                    let old_break_bb = self.break_bb.replace(main_after);
                    let old_break_depth = self.break_cleanup_depth.replace(catch_start_depth);
                    self.compile_stmts(catch_stmts, &[]);
                    self.break_bb = old_break_bb;
                    self.break_cleanup_depth = old_break_depth;
                    self.variables.remove(param_name);
                    self.var_types.remove(param_name);
                    if self.no_terminator() {
                        self.cleanup_to_depth(catch_start_depth);
                        self.builder.build_unconditional_branch(main_loop).unwrap();
                    }
                }

                // ── after_main ────────────────────────────────────────────────
                self.builder.position_at_end(main_after);

                // Pop signal recovery slot on clean exit.
                let exit_fn = self.module.get_function("kyte_anchor_exit").unwrap();
                self.builder.build_call(exit_fn, &[], "").unwrap();

                if self.no_terminator() {
                    self.builder
                        .build_return(Some(&i32_type.const_int(0, false)))
                        .unwrap();
                }
            }
        }
    }

    // ── Inline child anchor (with restart loop) ──────────────────────────────

    pub(super) fn emit_child_anchor(
        &mut self,
        child_name: &str,
        child_body: &[(Stmt, Span)],
        grandchildren: &[(TopLevel, Span)],
        params: &[Param],
        catch_param: Option<&str>,
        catch_body: Option<&[(Stmt, Span)]>,
    ) {
        if !self.no_terminator() {
            return;
        }
        let func = self.current_fn.unwrap();

        let child_loop = self
            .context
            .append_basic_block(func, &format!("anchor_loop_{}", child_name));
        let child_recover = self
            .context
            .append_basic_block(func, &format!("recover_{}", child_name));
        let child_after = self
            .context
            .append_basic_block(func, &format!("after_{}", child_name));
        let child_catch_bb = if catch_body.is_some() {
            Some(
                self.context
                    .append_basic_block(func, &format!("catch_{}", child_name)),
            )
        } else {
            None
        };

        let yield_alloca = self.build_alloca(&format!("{}_yield", child_name), &Ty::I64);
        self.builder
            .build_store(yield_alloca, self.i64_type().const_int(0, false))
            .unwrap();
        let kill_count_alloca = self.build_alloca(&format!("{}_kill_count", child_name), &Ty::I64);
        self.builder
            .build_store(kill_count_alloca, self.i64_type().const_int(0, false))
            .unwrap();
        let child_catch_msg_slot = if catch_body.is_some() {
            let slot = self.build_alloca(&format!("{}_catch_msg", child_name), &Ty::String);
            let empty = self.global_string_ptr("", "catch_empty");
            self.builder.build_store(slot, empty).unwrap();
            Some(slot)
        } else {
            None
        };

        self.builder.build_unconditional_branch(child_loop).unwrap();
        self.builder.position_at_end(child_loop);

        let enter_fn = self.module.get_function("kyte_anchor_enter").unwrap();
        let jmp_ret = self
            .builder
            .build_call(enter_fn, &[], &format!("{}_jmp", child_name))
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let jmp_signal = self
            .builder
            .build_int_compare(
                inkwell::IntPredicate::NE,
                jmp_ret,
                self.context.i32_type().const_int(0, false),
                &format!("{}_sig", child_name),
            )
            .unwrap();
        let body_start = self
            .context
            .append_basic_block(func, &format!("body_{}", child_name));
        self.builder
            .build_conditional_branch(jmp_signal, child_recover, body_start)
            .unwrap();
        self.builder.position_at_end(body_start);

        self.recovery_stack.push(child_recover);
        self.anchor_restart_bb_stack.push(child_loop);
        self.kill_cleanup_depth_stack
            .push(self.vault_scope_stack.len());
        self.yield_slot.push(yield_alloca);
        self.yield_merge_bb.push(child_after);
        self.kill_count_slot.push(kill_count_alloca);
        self.catch_bb_stack.push(child_catch_bb);
        self.catch_msg_slot_stack.push(child_catch_msg_slot);

        let child_vault_names = collect_vault_names(child_body);
        for vname in &child_vault_names {
            if let Some(&ptr) = self.variables.get(vname) {
                self.builder
                    .build_store(ptr, self.ptr_type().const_null())
                    .unwrap();
            }
        }

        let child_exp_vaults = self.save_vault_count(child_name);
        self.compile_stmts(child_body, params);

        for (gc, _) in grandchildren {
            if let TopLevel::Anchor {
                name: gc_name,
                kind: gc_kind,
                body: gc_body,
                children: gc_children,
                catch_param: gc_catch_param,
                catch_body: gc_catch_body,
                ..
            } = gc
            {
                match gc_kind {
                    AnchorKind::Thread => {
                        self.emit_thread_anchor(gc_name, gc_body, gc_children);
                    }
                    AnchorKind::Event(ev) => {
                        self.emit_event_anchor(gc_name, ev, gc_body);
                    }
                    _ => {
                        self.emit_child_anchor(
                            gc_name,
                            gc_body,
                            gc_children,
                            params,
                            gc_catch_param.as_deref(),
                            gc_catch_body.as_deref(),
                        );
                    }
                }
            }
        }

        if self.no_terminator() {
            self.builder
                .build_unconditional_branch(child_after)
                .unwrap();
        }

        // Signal recovery → restart
        self.builder.position_at_end(child_recover);
        self.emit_recovery_vault_assert(child_loop, child_exp_vaults, child_name);

        self.recovery_stack.pop();
        self.anchor_restart_bb_stack.pop();
        self.kill_cleanup_depth_stack.pop();
        self.yield_slot.pop();
        self.yield_merge_bb.pop();
        self.kill_count_slot.pop();
        self.catch_bb_stack.pop();
        self.catch_msg_slot_stack.pop();

        // Catch block (cold path)
        if let (Some(catch_bb), Some(catch_stmts)) = (child_catch_bb, catch_body) {
            self.builder.position_at_end(catch_bb);
            let param_name = catch_param.unwrap_or("_reason");
            if let Some(msg_slot) = child_catch_msg_slot {
                let msg_val = self
                    .builder
                    .build_load(self.ptr_type(), msg_slot, param_name)
                    .unwrap();
                let param_alloca = self.build_alloca(param_name, &Ty::String);
                self.builder.build_store(param_alloca, msg_val).unwrap();
                self.variables.insert(param_name.to_string(), param_alloca);
                self.var_types.insert(param_name.to_string(), Ty::String);
            }
            let catch_start_depth = self.vault_scope_stack.len();
            let old_break_bb = self.break_bb.replace(child_after);
            let old_break_depth = self.break_cleanup_depth.replace(catch_start_depth);
            self.compile_stmts(catch_stmts, params);
            self.break_bb = old_break_bb;
            self.break_cleanup_depth = old_break_depth;
            self.variables.remove(param_name);
            self.var_types.remove(param_name);
            if self.no_terminator() {
                self.cleanup_to_depth(catch_start_depth);
                self.builder.build_unconditional_branch(child_loop).unwrap();
            }
        }

        self.builder.position_at_end(child_after);
        let exit_fn = self.module.get_function("kyte_anchor_exit").unwrap();
        self.builder.build_call(exit_fn, &[], "").unwrap();
    }

    // ── Event anchor: register synchronous event handler ─────────────────────

    pub(super) fn emit_event_anchor(
        &mut self,
        anchor_name: &str,
        event_name: &str,
        body: &[(Stmt, Span)],
    ) {
        if !self.no_terminator() {
            return;
        }

        // Create a dedicated LLVM function for the handler body.
        // Signature: void __kyte_event_<name>(ptr payload)
        let body_fn_name = format!("__kyte_event_{}", anchor_name);
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[self.ptr_type().into()], false);
        let body_fn = self.module.add_function(&body_fn_name, fn_type, None);

        // Save outer codegen state
        let outer_fn = self.current_fn;
        let outer_vars = self.variables.clone();
        let outer_types = self.var_types.clone();
        let outer_vault_stack = self.vault_scope_stack.clone();
        let outer_freed = self.freed_vault_vars.clone();
        let outer_vlc = self.vault_live_count;
        let outer_recovery = std::mem::take(&mut self.recovery_stack);
        let outer_restart = std::mem::take(&mut self.anchor_restart_bb_stack);
        let outer_kill_depth = std::mem::take(&mut self.kill_cleanup_depth_stack);
        let outer_yield_slot = std::mem::take(&mut self.yield_slot);
        let outer_yield_merge = std::mem::take(&mut self.yield_merge_bb);
        let outer_kill_count = std::mem::take(&mut self.kill_count_slot);
        let outer_catch_bb = std::mem::take(&mut self.catch_bb_stack);
        let outer_catch_msg = std::mem::take(&mut self.catch_msg_slot_stack);
        let outer_break_bb = self.break_bb;
        let outer_break_depth = self.break_cleanup_depth;

        // Compile the event handler function body
        self.current_fn = Some(body_fn);
        self.variables.clear();
        self.var_types.clear();
        self.vault_scope_stack.clear();
        self.freed_vault_vars.clear();
        self.break_bb = None;
        self.break_cleanup_depth = None;

        let entry = self.context.append_basic_block(body_fn, "entry");
        let event_loop = self
            .context
            .append_basic_block(body_fn, &format!("event_loop_{}", anchor_name));
        let event_recover = self
            .context
            .append_basic_block(body_fn, &format!("event_recover_{}", anchor_name));
        let event_after = self
            .context
            .append_basic_block(body_fn, &format!("event_after_{}", anchor_name));

        self.builder.position_at_end(entry);

        // vault_live_count
        let vlc = self.build_alloca("vault_live_count", &Ty::I64);
        self.builder
            .build_store(vlc, self.i64_type().const_int(0, false))
            .unwrap();
        self.vault_live_count = Some(vlc);

        // Per-restart slots
        let yield_alloca = self.build_alloca(&format!("{}_yield", anchor_name), &Ty::I64);
        self.builder
            .build_store(yield_alloca, self.i64_type().const_int(0, false))
            .unwrap();
        let kill_count_alloca = self.build_alloca(&format!("{}_kill_count", anchor_name), &Ty::I64);
        self.builder
            .build_store(kill_count_alloca, self.i64_type().const_int(0, false))
            .unwrap();

        // _payload: alloca storing the function parameter (payload survives restarts)
        let payload_param = body_fn.get_nth_param(0).unwrap().into_pointer_value();
        let payload_alloca = self.build_alloca("_payload", &Ty::String);
        self.builder
            .build_store(payload_alloca, payload_param)
            .unwrap();

        self.builder.build_unconditional_branch(event_loop).unwrap();

        // Restart loop
        self.builder.position_at_end(event_loop);

        let enter_fn = self.module.get_function("kyte_anchor_enter").unwrap();
        let jmp_ret = self
            .builder
            .build_call(enter_fn, &[], &format!("{}_jmp", anchor_name))
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let jmp_sig = self
            .builder
            .build_int_compare(
                inkwell::IntPredicate::NE,
                jmp_ret,
                self.context.i32_type().const_int(0, false),
                &format!("{}_sig", anchor_name),
            )
            .unwrap();
        let body_start = self
            .context
            .append_basic_block(body_fn, &format!("event_body_{}", anchor_name));
        self.builder
            .build_conditional_branch(jmp_sig, event_recover, body_start)
            .unwrap();
        self.builder.position_at_end(body_start);

        // Inject _payload as a string variable visible in the handler body
        self.variables
            .insert("_payload".to_string(), payload_alloca);
        self.var_types.insert("_payload".to_string(), Ty::String);

        self.recovery_stack.push(event_recover);
        self.anchor_restart_bb_stack.push(event_loop);
        self.kill_cleanup_depth_stack
            .push(self.vault_scope_stack.len());
        self.yield_slot.push(yield_alloca);
        self.yield_merge_bb.push(event_after);
        self.kill_count_slot.push(kill_count_alloca);
        // No catch block at this level — Kill uses counter+escalation path
        self.catch_bb_stack.push(None);
        self.catch_msg_slot_stack.push(None);

        let exp_vaults = self.save_vault_count(anchor_name);
        self.compile_stmts(body, &[]);

        if self.no_terminator() {
            self.builder
                .build_unconditional_branch(event_after)
                .unwrap();
        }

        // Signal recovery → restart
        self.builder.position_at_end(event_recover);
        self.emit_recovery_vault_assert(event_loop, exp_vaults, anchor_name);

        self.recovery_stack.pop();
        self.anchor_restart_bb_stack.pop();
        self.kill_cleanup_depth_stack.pop();
        self.yield_slot.pop();
        self.yield_merge_bb.pop();
        self.kill_count_slot.pop();
        self.catch_bb_stack.pop();
        self.catch_msg_slot_stack.pop();

        self.builder.position_at_end(event_after);
        let exit_fn_rt = self.module.get_function("kyte_anchor_exit").unwrap();
        self.builder.build_call(exit_fn_rt, &[], "").unwrap();
        self.builder.build_return(None).unwrap();

        // Restore outer state
        self.current_fn = outer_fn;
        self.variables = outer_vars;
        self.var_types = outer_types;
        self.vault_scope_stack = outer_vault_stack;
        self.freed_vault_vars = outer_freed;
        self.vault_live_count = outer_vlc;
        self.recovery_stack = outer_recovery;
        self.anchor_restart_bb_stack = outer_restart;
        self.kill_cleanup_depth_stack = outer_kill_depth;
        self.yield_slot = outer_yield_slot;
        self.yield_merge_bb = outer_yield_merge;
        self.kill_count_slot = outer_kill_count;
        self.catch_bb_stack = outer_catch_bb;
        self.catch_msg_slot_stack = outer_catch_msg;
        self.break_bb = outer_break_bb;
        self.break_cleanup_depth = outer_break_depth;

        // In the outer function: register the handler for event_name
        if self.no_terminator() {
            let register_fn = self.module.get_function("kyte_register_event").unwrap();
            let ev_name_ptr =
                self.global_string_ptr(event_name, &format!("ev_name_{}", anchor_name));
            let fn_ptr = body_fn.as_global_value().as_pointer_value();
            self.builder
                .build_call(register_fn, &[ev_name_ptr.into(), fn_ptr.into()], "")
                .unwrap();
        }
    }

    // ── Thread anchor: spawn supervised OS thread ─────────────────────────────

    pub(super) fn emit_thread_anchor(
        &mut self,
        anchor_name: &str,
        body: &[(Stmt, Span)],
        _children: &[(TopLevel, Span)],
    ) {
        if !self.no_terminator() {
            return;
        }

        // Create a dedicated LLVM function for the anchor body.
        // Signature: void anchor_<name>(void*) — called by the C supervisor.
        let body_fn_name = format!("__kyte_thread_{}", anchor_name);
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[self.ptr_type().into()], false);
        let body_fn = self.module.add_function(&body_fn_name, fn_type, None);

        // Save outer codegen state
        let outer_fn = self.current_fn;
        let outer_vars = self.variables.clone();
        let outer_types = self.var_types.clone();
        let outer_vault_stack = self.vault_scope_stack.clone();
        let outer_freed = self.freed_vault_vars.clone();
        let outer_vlc = self.vault_live_count;
        let outer_recovery = std::mem::take(&mut self.recovery_stack);
        let outer_restart = std::mem::take(&mut self.anchor_restart_bb_stack);
        let outer_kill_depth = std::mem::take(&mut self.kill_cleanup_depth_stack);
        let outer_yield_slot = std::mem::take(&mut self.yield_slot);
        let outer_yield_merge = std::mem::take(&mut self.yield_merge_bb);
        let outer_kill_count = std::mem::take(&mut self.kill_count_slot);
        let outer_catch_bb_th = std::mem::take(&mut self.catch_bb_stack);
        let outer_catch_msg_th = std::mem::take(&mut self.catch_msg_slot_stack);
        let outer_break_bb = self.break_bb;
        let outer_break_depth = self.break_cleanup_depth;

        // Set up the thread body function
        self.current_fn = Some(body_fn);
        self.variables.clear();
        self.var_types.clear();
        self.vault_scope_stack.clear();
        self.freed_vault_vars.clear();
        self.break_bb = None;
        self.break_cleanup_depth = None;

        let entry = self.context.append_basic_block(body_fn, "entry");
        let thread_loop = self
            .context
            .append_basic_block(body_fn, "thread_anchor_loop");
        let thread_recover = self.context.append_basic_block(body_fn, "thread_recover");
        let thread_after = self.context.append_basic_block(body_fn, "thread_after");

        self.builder.position_at_end(entry);

        let vlc = self.build_alloca("vault_live_count", &Ty::I64);
        self.builder
            .build_store(vlc, self.i64_type().const_int(0, false))
            .unwrap();
        self.vault_live_count = Some(vlc);

        let yield_alloca = self.build_alloca(&format!("{}_yield", anchor_name), &Ty::I64);
        self.builder
            .build_store(yield_alloca, self.i64_type().const_int(0, false))
            .unwrap();
        let kill_count_alloca = self.build_alloca(&format!("{}_kill_count", anchor_name), &Ty::I64);
        self.builder
            .build_store(kill_count_alloca, self.i64_type().const_int(0, false))
            .unwrap();

        self.builder
            .build_unconditional_branch(thread_loop)
            .unwrap();

        // Thread loop (restart target)
        self.builder.position_at_end(thread_loop);

        let enter_fn = self.module.get_function("kyte_anchor_enter").unwrap();
        let jmp_ret = self
            .builder
            .build_call(enter_fn, &[], "th_jmp")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let jmp_sig = self
            .builder
            .build_int_compare(
                inkwell::IntPredicate::NE,
                jmp_ret,
                self.context.i32_type().const_int(0, false),
                "th_sig",
            )
            .unwrap();
        let body_start = self
            .context
            .append_basic_block(body_fn, "thread_body_start");
        self.builder
            .build_conditional_branch(jmp_sig, thread_recover, body_start)
            .unwrap();
        self.builder.position_at_end(body_start);

        self.recovery_stack.push(thread_recover);
        self.anchor_restart_bb_stack.push(thread_loop);
        self.kill_cleanup_depth_stack
            .push(self.vault_scope_stack.len());
        self.yield_slot.push(yield_alloca);
        self.yield_merge_bb.push(thread_after);
        self.kill_count_slot.push(kill_count_alloca);
        self.catch_bb_stack.push(None);
        self.catch_msg_slot_stack.push(None);

        let exp_vaults = self.save_vault_count(anchor_name);
        self.compile_stmts(body, &[]);

        if self.no_terminator() {
            self.builder
                .build_unconditional_branch(thread_after)
                .unwrap();
        }

        // Recovery → restart thread loop
        self.builder.position_at_end(thread_recover);
        self.emit_recovery_vault_assert(thread_loop, exp_vaults, anchor_name);

        self.recovery_stack.pop();
        self.anchor_restart_bb_stack.pop();
        self.kill_cleanup_depth_stack.pop();
        self.yield_slot.pop();
        self.yield_merge_bb.pop();
        self.kill_count_slot.pop();
        self.catch_bb_stack.pop();
        self.catch_msg_slot_stack.pop();

        self.builder.position_at_end(thread_after);
        let exit_fn_rt = self.module.get_function("kyte_anchor_exit").unwrap();
        self.builder.build_call(exit_fn_rt, &[], "").unwrap();
        self.builder.build_return(None).unwrap();

        // Restore outer state
        self.current_fn = outer_fn;
        self.variables = outer_vars;
        self.var_types = outer_types;
        self.vault_scope_stack = outer_vault_stack;
        self.freed_vault_vars = outer_freed;
        self.vault_live_count = outer_vlc;
        self.recovery_stack = outer_recovery;
        self.anchor_restart_bb_stack = outer_restart;
        self.kill_cleanup_depth_stack = outer_kill_depth;
        self.yield_slot = outer_yield_slot;
        self.yield_merge_bb = outer_yield_merge;
        self.kill_count_slot = outer_kill_count;
        self.catch_bb_stack = outer_catch_bb_th;
        self.catch_msg_slot_stack = outer_catch_msg_th;
        self.break_bb = outer_break_bb;
        self.break_cleanup_depth = outer_break_depth;

        // In the outer function: call kyte_spawn_thread_anchor
        if self.no_terminator() {
            let spawn_fn = self
                .module
                .get_function("kyte_spawn_thread_anchor")
                .unwrap();
            let fn_ptr = body_fn.as_global_value().as_pointer_value();
            let arg_null = self.ptr_type().const_null();
            let max_restarts = self.context.i32_type().const_int(3, false);
            let name_ptr = self.global_string_ptr(anchor_name, &format!("th_name_{}", anchor_name));
            self.builder
                .build_call(
                    spawn_fn,
                    &[
                        fn_ptr.into(),
                        arg_null.into(),
                        max_restarts.into(),
                        name_ptr.into(),
                    ],
                    "",
                )
                .unwrap();
        }
    }
}

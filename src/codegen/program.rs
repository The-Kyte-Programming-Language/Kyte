use std::collections::HashMap;

use inkwell::types::{BasicMetadataTypeEnum, BasicType};

use super::Codegen;
use crate::ast::*;

impl<'ctx> Codegen<'ctx> {
    pub fn compile(&mut self, program: &Program) {
        self.declare_printf();
        self.declare_exit_fn();
        self.declare_snprintf();
        self.declare_strlen();
        self.declare_malloc();
        self.declare_free_fn();
        self.declare_strcmp();

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
                // 간단한 상수 표현식만 지원 (리터럴)
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
            // impl 블록의 메서드를 TraitName_TypeName_method 로 등록
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
            // mod 블록의 함수를 modname_funcname 으로 등록
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
        // helper closure — compile a function body given qualified name + TopLevel::Function
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
            let entry = self.context.append_basic_block(func, "entry");
            self.builder.position_at_end(entry);

            // Vault 런타임 카운터 초기화
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

            // 암시적 반환
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
                let entry = self.context.append_basic_block(main_fn, "entry");
                let main_recover = self.context.append_basic_block(main_fn, "recover_main");
                let main_after = self.context.append_basic_block(main_fn, "after_main");
                self.builder.position_at_end(entry);

                // Vault 런타임 카운터 초기화
                let vlc = self.build_alloca("vault_live_count", &Ty::I64);
                self.builder
                    .build_store(vlc, self.i64_type().const_int(0, false))
                    .unwrap();
                self.vault_live_count = Some(vlc);

                let main_yield = self.build_alloca("main_yield", &Ty::I64);
                self.builder
                    .build_store(main_yield, self.i64_type().const_int(0, false))
                    .unwrap();
                let main_kill_count = self.build_alloca("main_kill_count", &Ty::I64);
                self.builder
                    .build_store(main_kill_count, self.i64_type().const_int(0, false))
                    .unwrap();

                self.recovery_stack.push(main_recover);
                self.kill_cleanup_depth_stack
                    .push(self.vault_scope_stack.len());
                self.yield_slot.push(main_yield);
                self.yield_merge_bb.push(main_after);
                self.kill_count_slot.push(main_kill_count);

                let main_exp_vaults = self.save_vault_count("main");

                self.compile_stmts(body, &[]);

                // 자식 앵커 본문도 인라인 (recovery 블록 포함)
                for (child, _) in children {
                    if let TopLevel::Anchor {
                        name: child_name,
                        body: child_body,
                        children: grandchildren,
                        ..
                    } = child
                    {
                        let func = self.current_fn.unwrap();
                        let child_bb = self
                            .context
                            .append_basic_block(func, &format!("anchor_{}", child_name));
                        let child_recover = self
                            .context
                            .append_basic_block(func, &format!("recover_{}", child_name));
                        let child_merge = self
                            .context
                            .append_basic_block(func, &format!("after_{}", child_name));

                        let yield_alloca =
                            self.build_alloca(&format!("{}_yield", child_name), &Ty::I64);
                        self.builder
                            .build_store(yield_alloca, self.i64_type().const_int(0, false))
                            .unwrap();
                        let kill_count_alloca =
                            self.build_alloca(&format!("{}_kill_count", child_name), &Ty::I64);
                        self.builder
                            .build_store(kill_count_alloca, self.i64_type().const_int(0, false))
                            .unwrap();

                        self.builder.build_unconditional_branch(child_bb).unwrap();
                        self.builder.position_at_end(child_bb);

                        self.recovery_stack.push(child_recover);
                        self.kill_cleanup_depth_stack
                            .push(self.vault_scope_stack.len());
                        self.yield_slot.push(yield_alloca);
                        self.yield_merge_bb.push(child_merge);
                        self.kill_count_slot.push(kill_count_alloca);

                        let child_exp_vaults = self.save_vault_count(child_name);

                        self.compile_stmts(child_body, &[]);

                        for (gc, _) in grandchildren {
                            if let TopLevel::Anchor {
                                name: gc_name,
                                body: gc_body,
                                ..
                            } = gc
                            {
                                if self.no_terminator() {
                                    let func = self.current_fn.unwrap();
                                    let gc_bb = self
                                        .context
                                        .append_basic_block(func, &format!("anchor_{}", gc_name));
                                    let gc_recover = self
                                        .context
                                        .append_basic_block(func, &format!("recover_{}", gc_name));
                                    let gc_merge = self
                                        .context
                                        .append_basic_block(func, &format!("after_{}", gc_name));

                                    let gc_yield =
                                        self.build_alloca(&format!("{}_yield", gc_name), &Ty::I64);
                                    self.builder
                                        .build_store(gc_yield, self.i64_type().const_int(0, false))
                                        .unwrap();
                                    let gc_kill_count = self
                                        .build_alloca(&format!("{}_kill_count", gc_name), &Ty::I64);
                                    self.builder
                                        .build_store(
                                            gc_kill_count,
                                            self.i64_type().const_int(0, false),
                                        )
                                        .unwrap();

                                    self.builder.build_unconditional_branch(gc_bb).unwrap();
                                    self.builder.position_at_end(gc_bb);

                                    self.recovery_stack.push(gc_recover);
                                    self.kill_cleanup_depth_stack
                                        .push(self.vault_scope_stack.len());
                                    self.yield_slot.push(gc_yield);
                                    self.yield_merge_bb.push(gc_merge);
                                    self.kill_count_slot.push(gc_kill_count);

                                    let gc_exp_vaults = self.save_vault_count(gc_name);

                                    self.compile_stmts(gc_body, &[]);

                                    if self.no_terminator() {
                                        self.builder.build_unconditional_branch(gc_merge).unwrap();
                                    }
                                    self.builder.position_at_end(gc_recover);
                                    self.emit_recovery_vault_assert(
                                        gc_merge,
                                        gc_exp_vaults,
                                        gc_name,
                                    );

                                    self.recovery_stack.pop();
                                    self.kill_cleanup_depth_stack.pop();
                                    self.yield_slot.pop();
                                    self.yield_merge_bb.pop();
                                    self.kill_count_slot.pop();

                                    self.builder.position_at_end(gc_merge);
                                }
                            }
                        }

                        if self.no_terminator() {
                            self.builder
                                .build_unconditional_branch(child_merge)
                                .unwrap();
                        }
                        self.builder.position_at_end(child_recover);
                        self.emit_recovery_vault_assert(child_merge, child_exp_vaults, child_name);

                        self.recovery_stack.pop();
                        self.kill_cleanup_depth_stack.pop();
                        self.yield_slot.pop();
                        self.yield_merge_bb.pop();
                        self.kill_count_slot.pop();

                        self.builder.position_at_end(child_merge);
                    }
                }

                if self.no_terminator() {
                    self.builder.build_unconditional_branch(main_after).unwrap();
                }
                self.builder.position_at_end(main_recover);
                self.emit_recovery_vault_assert(main_after, main_exp_vaults, "main");

                self.recovery_stack.pop();
                self.kill_cleanup_depth_stack.pop();
                self.yield_slot.pop();
                self.yield_merge_bb.pop();
                self.kill_count_slot.pop();

                self.builder.position_at_end(main_after);

                if self.no_terminator() {
                    self.builder
                        .build_return(Some(&i32_type.const_int(0, false)))
                        .unwrap();
                }
            }
        }
    }
}

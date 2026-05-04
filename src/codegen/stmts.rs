use inkwell::basic_block::BasicBlock;
use inkwell::values::BasicValueEnum;
use inkwell::IntPredicate;

use super::Codegen;
use crate::ast::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn compile_stmts(&mut self, stmts: &[(Stmt, Span)], params: &[Param]) {
        self.compile_stmts_inner(stmts, params, &[]);
    }

    pub(super) fn compile_stmts_inner(
        &mut self,
        stmts: &[(Stmt, Span)],
        params: &[Param],
        end_frees: &[String],
    ) {
        let start_depth = self.vault_scope_stack.len();
        self.vault_scope_stack.push(Vec::new());
        self.string_temp_stack.push(Vec::new());

        // Collect vault vars declared in this block (for liveness scheduling)
        let local_vault_vars: Vec<String> = stmts
            .iter()
            .filter_map(|(s, _)| {
                if let Stmt::VaultDecl { name, .. } = s { Some(name.clone()) } else { None }
            })
            .collect();
        let schedule = super::liveness::compute_schedule(stmts, &local_vault_vars);

        for (i, (stmt, _)) in stmts.iter().enumerate() {
            // Special handling for If-with-else to inject branch frees from liveness schedule
            if let Stmt::If { cond, then_body, else_body: Some(else_body) } = stmt {
                let func = self.current_fn.unwrap();
                let cond_val = self.compile_expr(cond, params).into_int_value();
                let then_bb = self.context.append_basic_block(func, "then");
                let else_bb = self.context.append_basic_block(func, "else");
                let merge_bb = self.context.append_basic_block(func, "merge");

                self.builder
                    .build_conditional_branch(cond_val, then_bb, else_bb)
                    .unwrap();

                // Save freed state before the if
                let pre_if_freed = self.freed_vault_vars.clone();

                // Compile then-branch with injected end-frees from parent schedule
                let then_extra: Vec<String> = schedule.then_vars(i).to_vec();
                self.freed_vault_vars = pre_if_freed.clone();
                self.builder.position_at_end(then_bb);
                self.compile_stmts_inner(then_body, params, &then_extra);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }
                let after_then_freed = self.freed_vault_vars.clone();

                // Compile else-branch with injected end-frees from parent schedule
                let else_extra: Vec<String> = schedule.else_vars(i).to_vec();
                self.freed_vault_vars = pre_if_freed.clone();
                self.builder.position_at_end(else_bb);
                self.compile_stmts_inner(else_body, params, &else_extra);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }
                let after_else_freed = self.freed_vault_vars.clone();

                // Post-if freed state = intersection (vars freed in both branches)
                self.freed_vault_vars = after_then_freed
                    .intersection(&after_else_freed)
                    .cloned()
                    .collect();

                self.builder.position_at_end(merge_bb);
            } else {
                self.compile_stmt(stmt, params);
            }

            // Emit scheduled frees for this statement index
            for var in schedule.after_vars(i) {
                self.free_vault_var(var);
            }

            if let Stmt::VaultDecl { name, .. } = stmt {
                self.register_vault_in_current_scope(name);
            }

            // break/return 후 더 이상 코드 생성하지 않음
            if !self.no_terminator() {
                break;
            }
        }

        // Emit end_frees before scope cleanup (for branch-injected frees)
        if self.no_terminator() {
            for var in end_frees {
                self.free_vault_var(var);
            }
        }

        // control-flow terminator(예: return/break)에서 이미 cleanup_to_depth가 수행되어
        // 현재 스코프가 제거된 경우가 있으므로, 스택 깊이로 안전하게 정리한다.
        if self.vault_scope_stack.len() == start_depth {
            return;
        }

        if self.no_terminator() {
            self.cleanup_current_scope();
        } else {
            self.vault_scope_stack.pop();
        }
    }

    pub(super) fn compile_stmt(&mut self, stmt: &Stmt, params: &[Param]) {
        match stmt {
            Stmt::VarDecl { ty, name, value } => {
                // A07: auto 타입 추론 — codegen에서도 guess_expr_ty로 실제 타입 결정
                let effective_ty = if *ty == Ty::Auto {
                    self.guess_expr_ty(value, params)
                } else {
                    ty.clone()
                };
                let alloca = self.build_alloca(name, &effective_ty);
                let val = self.compile_expr(value, params);
                let val = self.coerce_to_ty(val, &effective_ty);
                self.builder.build_store(alloca, val).unwrap();
                self.variables.insert(name.clone(), alloca);
                self.var_types.insert(name.clone(), effective_ty);
                // 배열 길이 추적
                if let Expr::ArrayLit(elems) = value {
                    self.array_lengths.insert(name.clone(), elems.len() as u64);
                }
            }

            Stmt::VaultDecl { ty, name, value } => {
                // Vault → heap allocation (malloc)
                let malloc = self.module.get_function("malloc").unwrap();
                let size_val = if let Ty::Array(ref inner) = ty {
                    // 배열: 요소 크기 × 요소 수
                    let elem_size = self.type_size_bytes(inner);
                    let count = if let Expr::ArrayLit(elems) = value {
                        elems.len() as u64
                    } else {
                        1
                    };
                    self.i64_type().const_int(elem_size * count, false)
                } else {
                    let elem_size = self.type_size_bytes(ty);
                    self.i64_type().const_int(elem_size, false)
                };
                let heap_ptr = self
                    .builder
                    .build_call(malloc, &[size_val.into()], "vault_heap")
                    .unwrap()
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_pointer_value();

                // NULL 체크 (C03)
                self.emit_null_check(heap_ptr, name);

                // 값 계산 후 heap에 저장
                let val = self.compile_expr(value, params);
                let val = self.coerce_to_ty(val, ty);
                self.builder.build_store(heap_ptr, val).unwrap();

                // 변수는 힙 포인터를 저장하는 alloca (pointer 타입)
                let alloca = self.build_alloca(name, &Ty::String); // ptr size
                self.builder.build_store(alloca, heap_ptr).unwrap();
                self.variables.insert(name.clone(), alloca);
                self.var_types.insert(name.clone(), ty.clone());
                self.vault_vars.insert(name.clone());
                self.freed_vault_vars.remove(name);
                // 런타임 vault 카운터 증가
                if let Some(counter) = self.vault_live_count {
                    let cur = self
                        .builder
                        .build_load(self.i64_type(), counter, "vlc")
                        .unwrap()
                        .into_int_value();
                    let next = self
                        .builder
                        .build_int_add(cur, self.i64_type().const_int(1, false), "vlc_inc")
                        .unwrap();
                    self.builder.build_store(counter, next).unwrap();
                }
                // 배열 길이 추적
                if let Expr::ArrayLit(elems) = value {
                    self.array_lengths.insert(name.clone(), elems.len() as u64);
                }
            }

            Stmt::Assign { name, value } => {
                let val = self.compile_expr(value, params);
                self.store_var(name, val);
            }

            Stmt::IndexAssign { name, index, value } => {
                let ty = self.guess_var_ty(name, params);
                if let Ty::Array(ref inner) = ty {
                    let data_ptr = self.load_var(name, &ty).into_pointer_value();
                    let idx = self.compile_expr(index, params).into_int_value();
                    // 런타임 배열 범위 검사 (C04)
                    if let Some(&arr_len) = self.array_lengths.get(name) {
                        self.emit_bounds_check(idx, arr_len, name);
                    }
                    let elem_llvm_ty = self.elem_llvm_type(inner);
                    let gep = unsafe {
                        self.builder
                            .build_gep(elem_llvm_ty, data_ptr, &[idx], "idx_ptr")
                            .unwrap()
                    };
                    let val = self.compile_expr(value, params);
                    self.builder.build_store(gep, val).unwrap();
                }
            }

            Stmt::FieldAssign { name, field, value } => {
                let ty = self.guess_var_ty(name, params);
                if let Ty::Struct(sname) = ty {
                    if let Some((idx, field_ty)) = self.struct_field_info(&sname, field) {
                        let base_ptr = if self.vault_vars.contains(name) {
                            self.builder
                                .build_load(self.ptr_type(), self.variables[name], "vptr")
                                .unwrap()
                                .into_pointer_value()
                        } else {
                            self.variables[name]
                        };
                        let field_ptr = self
                            .builder
                            .build_struct_gep(self.struct_types[&sname], base_ptr, idx, "field_ptr")
                            .unwrap();
                        let val = self.compile_expr(value, params);
                        let val = self.coerce_to_ty(val, &field_ty);
                        self.builder.build_store(field_ptr, val).unwrap();
                    }
                }
            }

            Stmt::CompoundAssign { name, op, value } => {
                let ty = self.guess_var_ty(name, params);
                let old = self.load_var(name, &ty);
                let rhs = self.compile_expr(value, params);
                let result = self.compile_binop(op, old, rhs, &ty);
                self.store_var(name, result);
            }

            Stmt::Return(Some(e)) => {
                self.cleanup_to_depth(0);
                let val = self.compile_expr(e, params);
                self.builder.build_return(Some(&val)).unwrap();
            }
            Stmt::Return(None) => {
                self.cleanup_to_depth(0);
                self.builder.build_return(None).unwrap();
            }

            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let func = self.current_fn.unwrap();
                let cond_val = self.compile_expr(cond, params).into_int_value();
                let then_bb = self.context.append_basic_block(func, "then");
                let else_bb = self.context.append_basic_block(func, "else");
                let merge_bb = self.context.append_basic_block(func, "merge");

                self.builder
                    .build_conditional_branch(cond_val, then_bb, else_bb)
                    .unwrap();

                // then
                self.builder.position_at_end(then_bb);
                self.compile_stmts(then_body, params);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }

                // else
                self.builder.position_at_end(else_bb);
                if let Some(else_stmts) = else_body {
                    self.compile_stmts(else_stmts, params);
                }
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }

                self.builder.position_at_end(merge_bb);
            }

            Stmt::Loop(body) => {
                let func = self.current_fn.unwrap();
                let loop_bb = self.context.append_basic_block(func, "loop");
                let after_bb = self.context.append_basic_block(func, "after_loop");

                let saved_break = self.break_bb;
                let saved_break_depth = self.break_cleanup_depth;
                let saved_continue = self.continue_bb;
                let saved_continue_depth = self.continue_cleanup_depth;
                self.break_bb = Some(after_bb);
                self.break_cleanup_depth = Some(self.vault_scope_stack.len());
                self.continue_bb = Some(loop_bb);
                self.continue_cleanup_depth = Some(self.vault_scope_stack.len());

                self.builder.build_unconditional_branch(loop_bb).unwrap();
                self.builder.position_at_end(loop_bb);
                self.compile_stmts(body, params);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(loop_bb).unwrap();
                }

                self.break_bb = saved_break;
                self.break_cleanup_depth = saved_break_depth;
                self.continue_bb = saved_continue;
                self.continue_cleanup_depth = saved_continue_depth;
                self.builder.position_at_end(after_bb);
            }

            Stmt::While { cond, body } => {
                let func = self.current_fn.unwrap();
                let cond_bb = self.context.append_basic_block(func, "while_cond");
                let body_bb = self.context.append_basic_block(func, "while_body");
                let after_bb = self.context.append_basic_block(func, "while_after");

                let saved_break = self.break_bb;
                let saved_break_depth = self.break_cleanup_depth;
                let saved_continue = self.continue_bb;
                let saved_continue_depth = self.continue_cleanup_depth;
                self.break_bb = Some(after_bb);
                self.break_cleanup_depth = Some(self.vault_scope_stack.len());
                self.continue_bb = Some(cond_bb);
                self.continue_cleanup_depth = Some(self.vault_scope_stack.len());

                self.builder.build_unconditional_branch(cond_bb).unwrap();

                // condition
                self.builder.position_at_end(cond_bb);
                let cond_val = self.compile_expr(cond, params).into_int_value();
                self.builder
                    .build_conditional_branch(cond_val, body_bb, after_bb)
                    .unwrap();

                // body
                self.builder.position_at_end(body_bb);
                self.compile_stmts(body, params);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                }

                self.break_bb = saved_break;
                self.break_cleanup_depth = saved_break_depth;
                self.continue_bb = saved_continue;
                self.continue_cleanup_depth = saved_continue_depth;
                self.builder.position_at_end(after_bb);
            }

            Stmt::For {
                var,
                from,
                to,
                body,
            } => {
                let func = self.current_fn.unwrap();
                let loop_bb = self.context.append_basic_block(func, "for_body");
                let inc_bb = self.context.append_basic_block(func, "for_inc");
                let after_bb = self.context.append_basic_block(func, "for_after");

                // 초기값
                let start_val = self.compile_expr(from, params).into_int_value();
                let end_val = self.compile_expr(to, params).into_int_value();

                let saved_break = self.break_bb;
                let saved_break_depth = self.break_cleanup_depth;
                let saved_continue = self.continue_bb;
                let saved_continue_depth = self.continue_cleanup_depth;
                self.break_bb = Some(after_bb);
                self.break_cleanup_depth = Some(self.vault_scope_stack.len());
                self.continue_bb = Some(inc_bb);
                self.continue_cleanup_depth = Some(self.vault_scope_stack.len());

                // alloca for loop var
                let alloca = self.build_alloca(var, &Ty::Int);
                self.builder.build_store(alloca, start_val).unwrap();
                self.variables.insert(var.clone(), alloca);
                self.var_types.insert(var.clone(), Ty::Int);

                // 진입 조건
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::SLT, start_val, end_val, "for_cond")
                    .unwrap();
                self.builder
                    .build_conditional_branch(cond, loop_bb, after_bb)
                    .unwrap();

                // body
                self.builder.position_at_end(loop_bb);
                self.compile_stmts(body, params);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(inc_bb).unwrap();
                }

                // increment block (continue target)
                self.builder.position_at_end(inc_bb);
                let cur = self
                    .builder
                    .build_load(self.i64_type(), alloca, var)
                    .unwrap()
                    .into_int_value();
                let next = self
                    .builder
                    .build_int_add(cur, self.i64_type().const_int(1, false), "next")
                    .unwrap();
                self.builder.build_store(alloca, next).unwrap();
                let loop_cond = self
                    .builder
                    .build_int_compare(IntPredicate::SLT, next, end_val, "for_cond")
                    .unwrap();
                self.builder
                    .build_conditional_branch(loop_cond, loop_bb, after_bb)
                    .unwrap();

                self.break_bb = saved_break;
                self.break_cleanup_depth = saved_break_depth;
                self.continue_bb = saved_continue;
                self.continue_cleanup_depth = saved_continue_depth;
                self.builder.position_at_end(after_bb);
            }

            Stmt::Break => {
                if let Some(depth) = self.break_cleanup_depth {
                    self.cleanup_to_depth(depth);
                }
                if let Some(bb) = self.break_bb {
                    self.builder.build_unconditional_branch(bb).unwrap();
                }
            }

            Stmt::Continue => {
                if let Some(depth) = self.continue_cleanup_depth {
                    self.cleanup_to_depth(depth);
                }
                if let Some(bb) = self.continue_bb {
                    self.builder.build_unconditional_branch(bb).unwrap();
                }
            }

            Stmt::Exit => {
                self.cleanup_to_depth(0);
                let exit_fn = self.module.get_function("exit").unwrap();
                self.builder
                    .build_call(
                        exit_fn,
                        &[self.context.i32_type().const_int(0, false).into()],
                        "",
                    )
                    .unwrap();
                self.builder.build_unreachable().unwrap();
            }

            Stmt::Kill(msg) => {
                // ── Compile kill message as i8* ──────────────────────────────
                let kill_msg_ptr = if let Some(e) = msg {
                    let ty = self.guess_expr_ty(e, params);
                    let val = self.compile_expr(e, params);
                    if matches!(ty, Ty::String) {
                        val.into_pointer_value()
                    } else {
                        // non-string: print for visibility, store empty in catch slot
                        self.emit_print(val, Some(&ty));
                        self.global_string_ptr("", "kill_msg_empty")
                    }
                } else {
                    self.global_string_ptr("", "kill_msg_empty")
                };

                // ── Optimization: catch block → direct jump, no counter ──────
                let maybe_catch = self.catch_bb_stack.last().and_then(|x| *x);

                if let Some(catch_bb) = maybe_catch {
                    // Store message for catch parameter
                    if let Some(Some(msg_slot)) = self.catch_msg_slot_stack.last().copied() {
                        self.builder.build_store(msg_slot, kill_msg_ptr).unwrap();
                    }
                    let cleanup_depth = self.kill_cleanup_depth_stack.last().copied().unwrap_or(0);
                    self.cleanup_to_depth(cleanup_depth);
                    self.builder.build_unconditional_branch(catch_bb).unwrap();
                } else {
                    // ── No catch: log Kill via structured kyte_log_kill ───────
                    let anc_name = self
                        .anchor_name_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(|| "main".to_string());
                    let anc_ptr = self.global_string_ptr(&anc_name, "kill_anc_name");
                    let log_kill = self.module.get_function("kyte_log_kill").unwrap();
                    self.builder
                        .build_call(log_kill, &[anc_ptr.into(), kill_msg_ptr.into()], "")
                        .unwrap();

                    if let Some(&recovery_bb) = self.recovery_stack.last() {
                        let normal_depth =
                            self.kill_cleanup_depth_stack.last().copied().unwrap_or(0);

                        if let Some(&counter_ptr) = self.kill_count_slot.last() {
                            // ── Increment kill counter ───────────────────────
                            let cur = self
                                .builder
                                .build_load(self.i64_type(), counter_ptr, "kill_count")
                                .unwrap()
                                .into_int_value();
                            let next = self
                                .builder
                                .build_int_add(
                                    cur,
                                    self.i64_type().const_int(1, false),
                                    "kill_count_next",
                                )
                                .unwrap();
                            self.builder.build_store(counter_ptr, next).unwrap();

                            // ── Decide: restart or escalate ──────────────────
                            let escalate_cond = self
                                .builder
                                .build_int_compare(
                                    IntPredicate::UGE,
                                    next,
                                    self.i64_type().const_int(3, false),
                                    "kill_escalate_cond",
                                )
                                .unwrap();

                            let normal_bb = self
                                .context
                                .append_basic_block(self.current_fn.unwrap(), "kill_restart");
                            let escalated_bb = self
                                .context
                                .append_basic_block(self.current_fn.unwrap(), "kill_escalate");
                            self.builder
                                .build_conditional_branch(escalate_cond, escalated_bb, normal_bb)
                                .unwrap();

                            // ── normal_bb: log restart, cleanup, restart ──────
                            self.builder.position_at_end(normal_bb);
                            let log_restart =
                                self.module.get_function("kyte_log_restart").unwrap();
                            let rst_anc = self
                                .anchor_name_stack
                                .last()
                                .cloned()
                                .unwrap_or_else(|| "main".to_string());
                            let rst_ptr =
                                self.global_string_ptr(&rst_anc, "rst_anc_name");
                            let attempt_i32 = self
                                .builder
                                .build_int_truncate(
                                    next,
                                    self.context.i32_type(),
                                    "attempt_i32",
                                )
                                .unwrap();
                            self.builder
                                .build_call(
                                    log_restart,
                                    &[rst_ptr.into(), attempt_i32.into()],
                                    "",
                                )
                                .unwrap();
                            self.cleanup_to_depth(normal_depth);
                            self.builder
                                .build_unconditional_branch(recovery_bb)
                                .unwrap();

                            // ── escalated_bb: escalate to parent or Exit ──────
                            self.builder.position_at_end(escalated_bb);
                            if self.recovery_stack.len() >= 2 {
                                let parent_recovery =
                                    self.recovery_stack[self.recovery_stack.len() - 2];
                                let parent_depth = self
                                    .kill_cleanup_depth_stack
                                    .get(self.kill_cleanup_depth_stack.len() - 2)
                                    .copied()
                                    .unwrap_or(0);
                                let log_esc =
                                    self.module.get_function("kyte_log_escalate").unwrap();
                                let esc_anc = self
                                    .anchor_name_stack
                                    .last()
                                    .cloned()
                                    .unwrap_or_else(|| "main".to_string());
                                let anc_ptr =
                                    self.global_string_ptr(&esc_anc, "esc_anc_name");
                                self.builder
                                    .build_call(log_esc, &[anc_ptr.into()], "")
                                    .unwrap();
                                self.cleanup_to_depth(parent_depth);
                                self.builder
                                    .build_unconditional_branch(parent_recovery)
                                    .unwrap();
                            } else {
                                self.cleanup_to_depth(0);
                                let exit_fn = self.module.get_function("exit").unwrap();
                                self.builder
                                    .build_call(
                                        exit_fn,
                                        &[self.context.i32_type().const_int(1, false).into()],
                                        "",
                                    )
                                    .unwrap();
                                self.builder.build_unreachable().unwrap();
                            }
                        } else {
                            self.cleanup_to_depth(normal_depth);
                            self.builder
                                .build_unconditional_branch(recovery_bb)
                                .unwrap();
                        }
                    }
                }
            }

            Stmt::Yield(e) => {
                let val = self.compile_expr(e, params);
                // yield 슬롯이 있으면 값 저장 후 앵커 종료 블록으로 점프
                if let (Some(&slot), Some(&merge_bb)) =
                    (self.yield_slot.last(), self.yield_merge_bb.last())
                {
                    let coerced = self.coerce_to_ty(val, &Ty::I64);
                    self.builder.build_store(slot, coerced).unwrap();
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }
            }

            Stmt::Print(args) => {
                for a in args {
                    let val = self.compile_expr(a, params);
                    let ty = self.guess_expr_ty(a, params);
                    self.emit_print(val, Some(&ty));
                }
            }

            Stmt::Assert { cond, message } => {
                let cond_val = self.compile_expr(cond, params).into_int_value();
                let func = self.current_fn.unwrap();
                let fail_bb = self.context.append_basic_block(func, "assert_fail");
                let ok_bb = self.context.append_basic_block(func, "assert_ok");
                self.builder
                    .build_conditional_branch(cond_val, ok_bb, fail_bb)
                    .unwrap();

                self.builder.position_at_end(fail_bb);
                self.declare_printf();
                let printf = self.module.get_function("printf").unwrap();
                if let Some(msg_expr) = message {
                    let msg_val = self.compile_expr(msg_expr, params);
                    let msg_ptr =
                        self.to_string_ptr(msg_val, &self.guess_expr_ty(msg_expr, params));
                    let fmt = self.global_string_ptr("assertion failed: %s\\n", "assert_msg_fmt");
                    self.builder
                        .build_call(printf, &[fmt.into(), msg_ptr.into()], "")
                        .unwrap();
                } else {
                    let fmt = self.global_string_ptr("assertion failed\\n", "assert_fmt");
                    self.builder.build_call(printf, &[fmt.into()], "").unwrap();
                }
                let exit_fn = self.module.get_function("exit").unwrap();
                self.builder
                    .build_call(
                        exit_fn,
                        &[self.context.i32_type().const_int(1, false).into()],
                        "",
                    )
                    .unwrap();
                self.builder.build_unreachable().unwrap();

                self.builder.position_at_end(ok_bb);
            }

            Stmt::InlineAnchor {
                name,
                body,
                catch_param,
                catch_body,
                ..
            } => {
                let func = self.current_fn.unwrap();
                let has_catch = catch_body.is_some();

                // ── Blocks ───────────────────────────────────────────────────
                let anchor_loop_bb = self
                    .context
                    .append_basic_block(func, &format!("anchor_loop_{}", name));
                let recovery_bb = self
                    .context
                    .append_basic_block(func, &format!("recover_{}", name));
                let merge_bb = self
                    .context
                    .append_basic_block(func, &format!("after_{}", name));
                // catch block placed after hot path → cold block layout
                let catch_bb_opt = if has_catch {
                    Some(
                        self.context
                            .append_basic_block(func, &format!("catch_{}", name)),
                    )
                } else {
                    None
                };

                // ── Allocas in entry block (persist across restarts) ──────────
                let yield_alloca = self.build_alloca(&format!("{}_yield", name), &Ty::I64);
                self.builder
                    .build_store(yield_alloca, self.i64_type().const_int(0, false))
                    .unwrap();
                // kill_count only needed when no catch (optimization: eliminates
                // load/add/store/icmp/br on every Kill when catch handles recovery)
                let kill_count_alloca =
                    self.build_alloca(&format!("{}_kill_count", name), &Ty::I64);
                self.builder
                    .build_store(kill_count_alloca, self.i64_type().const_int(0, false))
                    .unwrap();

                // catch message slot: i8* alloca, default empty string
                let catch_msg_slot = if has_catch {
                    let slot = self.build_alloca(&format!("{}_catch_msg", name), &Ty::String);
                    let empty = self.global_string_ptr("", "catch_empty");
                    self.builder.build_store(slot, empty).unwrap();
                    Some(slot)
                } else {
                    None
                };

                // ── Enter restart loop ────────────────────────────────────────
                self.builder
                    .build_unconditional_branch(anchor_loop_bb)
                    .unwrap();
                self.builder.position_at_end(anchor_loop_bb);

                // sigsetjmp — handles signal recovery (SIGFPE, etc.)
                let enter_fn = self.module.get_function("kyte_anchor_enter").unwrap();
                let jmp_ret = self
                    .builder
                    .build_call(enter_fn, &[], &format!("{}_jmp", name))
                    .unwrap()
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_int_value();
                let jmp_sig = self
                    .builder
                    .build_int_compare(
                        IntPredicate::NE,
                        jmp_ret,
                        self.context.i32_type().const_int(0, false),
                        &format!("{}_sig", name),
                    )
                    .unwrap();
                let body_start = self
                    .context
                    .append_basic_block(func, &format!("body_start_{}", name));
                self.builder
                    .build_conditional_branch(jmp_sig, recovery_bb, body_start)
                    .unwrap();
                self.builder.position_at_end(body_start);

                // ── Push supervisor stacks ────────────────────────────────────
                self.recovery_stack.push(recovery_bb);
                self.anchor_restart_bb_stack.push(anchor_loop_bb);
                self.kill_cleanup_depth_stack
                    .push(self.vault_scope_stack.len());
                self.yield_slot.push(yield_alloca);
                self.yield_merge_bb.push(merge_bb);
                self.kill_count_slot.push(kill_count_alloca);
                self.catch_bb_stack.push(catch_bb_opt);
                self.catch_msg_slot_stack.push(catch_msg_slot);

                let inline_exp_vaults = self.save_vault_count(name);
                self.anchor_name_stack.push(name.clone());
                self.compile_stmts(body, params);

                // Normal exit → merge
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }

                // ── Signal recovery block (SIGFPE etc.) → restart ─────────────
                self.builder.position_at_end(recovery_bb);
                self.emit_recovery_vault_assert(anchor_loop_bb, inline_exp_vaults, name);

                // ── Pop supervisor stacks ─────────────────────────────────────
                self.anchor_name_stack.pop();
                self.recovery_stack.pop();
                self.anchor_restart_bb_stack.pop();
                self.kill_cleanup_depth_stack.pop();
                self.yield_slot.pop();
                self.yield_merge_bb.pop();
                self.kill_count_slot.pop();
                self.catch_bb_stack.pop();
                self.catch_msg_slot_stack.pop();

                // ── Catch block (cold path) ───────────────────────────────────
                if let (Some(catch_bb), Some(catch_stmts)) = (catch_bb_opt, catch_body.as_deref()) {
                    self.builder.position_at_end(catch_bb);

                    // Bind catch parameter as a local string variable
                    let param_name = catch_param.as_deref().unwrap_or("_reason");
                    if let Some(msg_slot) = catch_msg_slot {
                        let msg_val = self
                            .builder
                            .build_load(self.ptr_type(), msg_slot, param_name)
                            .unwrap();
                        let param_alloca = self.build_alloca(param_name, &Ty::String);
                        self.builder.build_store(param_alloca, msg_val).unwrap();
                        self.variables.insert(param_name.to_string(), param_alloca);
                        self.var_types.insert(param_name.to_string(), Ty::String);
                    }

                    // break inside catch → exit anchor (jump to merge_bb)
                    let catch_start_depth = self.vault_scope_stack.len();
                    let old_break_bb = self.break_bb.replace(merge_bb);
                    let old_break_depth = self.break_cleanup_depth.replace(catch_start_depth);

                    self.compile_stmts(catch_stmts, params);

                    self.break_bb = old_break_bb;
                    self.break_cleanup_depth = old_break_depth;

                    // Remove catch param from scope
                    self.variables.remove(param_name);
                    self.var_types.remove(param_name);

                    // Fallthrough (no break) → restart anchor
                    if self.no_terminator() {
                        self.cleanup_to_depth(catch_start_depth);
                        self.builder
                            .build_unconditional_branch(anchor_loop_bb)
                            .unwrap();
                    }
                }

                // ── After anchor: pop signal slot on clean exit ───────────────
                self.builder.position_at_end(merge_bb);
                let exit_fn_rt = self.module.get_function("kyte_anchor_exit").unwrap();
                self.builder.build_call(exit_fn_rt, &[], "").unwrap();
            }

            Stmt::Match { expr, arms } => {
                let func = self.current_fn.unwrap();
                let val = self.compile_expr(expr, params);
                let expr_ty = self.guess_expr_ty(expr, params);
                let merge_bb = self.context.append_basic_block(func, "match_merge");

                if let Ty::Enum(ref ename) = expr_ty {
                    // ── enum match ──
                    // Check whether any arm has a guard or there are duplicate variant patterns.
                    // If so, use a sequential if-chain (tag compare per arm) to support guards
                    // correctly. Otherwise use the faster LLVM switch.
                    let has_any_guard = arms.iter().any(|a| a.guard.is_some());
                    let has_duplicate_variants = {
                        let mut seen = std::collections::HashSet::new();
                        arms.iter().any(|a| {
                            if let Pattern::EnumVariant { variant, .. } = &a.pattern {
                                !seen.insert(variant.clone())
                            } else {
                                false
                            }
                        })
                    };

                    let enum_struct = val.into_struct_value();
                    let tag = self
                        .builder
                        .build_extract_value(enum_struct, 0, "enum_tag")
                        .unwrap()
                        .into_int_value();
                    let ename_clone = ename.clone();

                    if has_any_guard || has_duplicate_variants {
                        // ── Sequential if-chain for enum arms with guards ──
                        let mut remaining_bb =
                            self.context.append_basic_block(func, "match_else_0");
                        self.builder
                            .build_unconditional_branch(remaining_bb)
                            .unwrap();

                        for (i, arm) in arms.iter().enumerate() {
                            self.builder.position_at_end(remaining_bb);
                            let arm_bb = self
                                .context
                                .append_basic_block(func, &format!("match_arm_{}", i));
                            let next_bb = if i + 1 < arms.len() {
                                self.context
                                    .append_basic_block(func, &format!("match_else_{}", i + 1))
                            } else {
                                merge_bb
                            };

                            // Pattern check
                            match &arm.pattern {
                                Pattern::EnumVariant { variant, .. } => {
                                    if let Some(tags) = self.enum_variant_tags.get(&ename_clone) {
                                        if let Some(&tv) = tags.get(variant) {
                                            let tag_val = self
                                                .context
                                                .i32_type()
                                                .const_int(tv as u64, false);
                                            let cond = self
                                                .builder
                                                .build_int_compare(
                                                    IntPredicate::EQ,
                                                    tag,
                                                    tag_val,
                                                    "tag_cmp",
                                                )
                                                .unwrap();
                                            self.builder
                                                .build_conditional_branch(cond, arm_bb, next_bb)
                                                .unwrap();
                                        } else {
                                            self.builder
                                                .build_unconditional_branch(next_bb)
                                                .unwrap();
                                        }
                                    } else {
                                        self.builder.build_unconditional_branch(next_bb).unwrap();
                                    }
                                }
                                Pattern::Wildcard | Pattern::Binding(_) => {
                                    self.builder.build_unconditional_branch(arm_bb).unwrap();
                                }
                                _ => {
                                    self.builder.build_unconditional_branch(next_bb).unwrap();
                                }
                            }

                            self.builder.position_at_end(arm_bb);

                            // Extract payload binding if needed
                            if let Pattern::EnumVariant {
                                variant,
                                binding: Some(bind_name),
                                ..
                            } = &arm.pattern
                            {
                                if let Some(variants) =
                                    self.enum_defs.get(&ename_clone).cloned()
                                {
                                    if let Some(v) =
                                        variants.iter().find(|v| v.name == *variant)
                                    {
                                        if let Some(ref payload_ty) = v.ty {
                                            let enum_alloca = self.build_alloca(
                                                &format!("match_enum_{}", i),
                                                &Ty::Enum(ename_clone.clone()),
                                            );
                                            self.builder.build_store(enum_alloca, val).unwrap();
                                            let enum_st = self.enum_types[&ename_clone];
                                            let payload_ptr = self
                                                .builder
                                                .build_struct_gep(
                                                    enum_st,
                                                    enum_alloca,
                                                    1,
                                                    "payload_ptr",
                                                )
                                                .unwrap();
                                            let payload_llvm_ty = self.ty_to_basic(payload_ty);
                                            let payload_val = self
                                                .builder
                                                .build_load(
                                                    payload_llvm_ty,
                                                    payload_ptr,
                                                    bind_name,
                                                )
                                                .unwrap();
                                            let bind_alloca =
                                                self.build_alloca(bind_name, payload_ty);
                                            self.builder
                                                .build_store(bind_alloca, payload_val)
                                                .unwrap();
                                            self.variables.insert(bind_name.clone(), bind_alloca);
                                            self.var_types
                                                .insert(bind_name.clone(), payload_ty.clone());
                                        }
                                    }
                                }
                            }

                            // Guard check: if guard fails, skip to next arm
                            if let Some(guard_expr) = &arm.guard {
                                let guard_pass_bb =
                                    self.context.append_basic_block(func, "guard_pass");
                                let guard_val =
                                    self.compile_expr(guard_expr, params).into_int_value();
                                self.builder
                                    .build_conditional_branch(guard_val, guard_pass_bb, next_bb)
                                    .unwrap();
                                self.builder.position_at_end(guard_pass_bb);
                            }

                            self.compile_stmts(&arm.body, params);
                            if self.no_terminator() {
                                self.builder.build_unconditional_branch(merge_bb).unwrap();
                            }
                            remaining_bb = next_bb;
                        }

                        if remaining_bb != merge_bb {
                            self.builder.position_at_end(remaining_bb);
                            if self.no_terminator() {
                                self.builder.build_unconditional_branch(merge_bb).unwrap();
                            }
                        }
                    } else {
                        // ── Fast path: LLVM switch on tag (no guards, no duplicates) ──
                        // Create arm basic blocks upfront
                        let arm_bbs: Vec<BasicBlock<'ctx>> = (0..arms.len())
                            .map(|i| {
                                self.context
                                    .append_basic_block(func, &format!("match_arm_{}", i))
                            })
                            .collect();
                        let default_bb = self.context.append_basic_block(func, "match_default");

                        // Collect switch cases
                        let mut cases: Vec<(inkwell::values::IntValue<'ctx>, BasicBlock<'ctx>)> =
                            Vec::new();
                        let mut wildcard_idx: Option<usize> = None;
                        for (i, arm) in arms.iter().enumerate() {
                            match &arm.pattern {
                                Pattern::EnumVariant { variant, .. } => {
                                    if let Some(tags) = self.enum_variant_tags.get(&ename_clone) {
                                        if let Some(&tv) = tags.get(variant) {
                                            cases.push((
                                                self.context
                                                    .i32_type()
                                                    .const_int(tv as u64, false),
                                                arm_bbs[i],
                                            ));
                                        }
                                    }
                                }
                                Pattern::Wildcard | Pattern::Binding(_) => {
                                    wildcard_idx = Some(i);
                                }
                                _ => {}
                            }
                        }

                        // Emit switch (tag → arm block)
                        let actual_default =
                            wildcard_idx.map(|i| arm_bbs[i]).unwrap_or(default_bb);
                        self.builder
                            .build_switch(tag, actual_default, &cases)
                            .unwrap();

                        // Compile each arm
                        for (i, arm) in arms.iter().enumerate() {
                            self.builder.position_at_end(arm_bbs[i]);

                            // Extract payload binding if needed
                            if let Pattern::EnumVariant {
                                variant,
                                binding: Some(bind_name),
                                ..
                            } = &arm.pattern
                            {
                                if let Some(variants) =
                                    self.enum_defs.get(&ename_clone).cloned()
                                {
                                    if let Some(v) =
                                        variants.iter().find(|v| v.name == *variant)
                                    {
                                        if let Some(ref payload_ty) = v.ty {
                                            let enum_alloca = self.build_alloca(
                                                &format!("match_enum_{}", i),
                                                &Ty::Enum(ename_clone.clone()),
                                            );
                                            self.builder.build_store(enum_alloca, val).unwrap();
                                            let enum_st = self.enum_types[&ename_clone];
                                            let payload_ptr = self
                                                .builder
                                                .build_struct_gep(
                                                    enum_st,
                                                    enum_alloca,
                                                    1,
                                                    "payload_ptr",
                                                )
                                                .unwrap();
                                            let payload_llvm_ty = self.ty_to_basic(payload_ty);
                                            let payload_val = self
                                                .builder
                                                .build_load(
                                                    payload_llvm_ty,
                                                    payload_ptr,
                                                    bind_name,
                                                )
                                                .unwrap();
                                            let bind_alloca =
                                                self.build_alloca(bind_name, payload_ty);
                                            self.builder
                                                .build_store(bind_alloca, payload_val)
                                                .unwrap();
                                            self.variables.insert(bind_name.clone(), bind_alloca);
                                            self.var_types
                                                .insert(bind_name.clone(), payload_ty.clone());
                                        }
                                    }
                                }
                            }

                            self.compile_stmts(&arm.body, params);
                            if self.no_terminator() {
                                self.builder.build_unconditional_branch(merge_bb).unwrap();
                            }
                        }

                        // Default block → merge (always needs a terminator)
                        self.builder.position_at_end(default_bb);
                        if self.no_terminator() {
                            self.builder.build_unconditional_branch(merge_bb).unwrap();
                        }
                    }

                    self.builder.position_at_end(merge_bb);
                } else {
                    // ── value match: if-else chain ──
                    let mut remaining_bb = self.context.append_basic_block(func, "match_else_0");
                    self.builder
                        .build_unconditional_branch(remaining_bb)
                        .unwrap();

                    for (i, arm) in arms.iter().enumerate() {
                        self.builder.position_at_end(remaining_bb);
                        let arm_bb = self
                            .context
                            .append_basic_block(func, &format!("match_arm_{}", i));
                        let next_bb = if i + 1 < arms.len() {
                            self.context
                                .append_basic_block(func, &format!("match_else_{}", i + 1))
                        } else {
                            merge_bb
                        };

                        match &arm.pattern {
                            Pattern::IntLit(n) => {
                                let cmp_val = self.i64_type().const_int(*n as u64, true);
                                let cond = self
                                    .builder
                                    .build_int_compare(
                                        IntPredicate::EQ,
                                        val.into_int_value(),
                                        cmp_val,
                                        "mcmp",
                                    )
                                    .unwrap();
                                self.builder
                                    .build_conditional_branch(cond, arm_bb, next_bb)
                                    .unwrap();
                            }
                            Pattern::Bool(b) => {
                                let cmp_val =
                                    self.bool_type().const_int(if *b { 1 } else { 0 }, false);
                                let cond = self
                                    .builder
                                    .build_int_compare(
                                        IntPredicate::EQ,
                                        val.into_int_value(),
                                        cmp_val,
                                        "mcmp",
                                    )
                                    .unwrap();
                                self.builder
                                    .build_conditional_branch(cond, arm_bb, next_bb)
                                    .unwrap();
                            }
                            Pattern::StringLit(s) => {
                                self.declare_strcmp();
                                let strcmp_fn = self.module.get_function("strcmp").unwrap();
                                let str_ptr = self.global_string_ptr(s, &format!("mstr_{}", i));
                                let cmp_result = self
                                    .builder
                                    .build_call(strcmp_fn, &[val.into(), str_ptr.into()], "scmp")
                                    .unwrap()
                                    .try_as_basic_value()
                                    .basic()
                                    .unwrap()
                                    .into_int_value();
                                let zero = self.context.i32_type().const_int(0, false);
                                let cond = self
                                    .builder
                                    .build_int_compare(IntPredicate::EQ, cmp_result, zero, "mcmp")
                                    .unwrap();
                                self.builder
                                    .build_conditional_branch(cond, arm_bb, next_bb)
                                    .unwrap();
                            }
                            Pattern::Wildcard => {
                                self.builder.build_unconditional_branch(arm_bb).unwrap();
                            }
                            Pattern::Binding(_) => {
                                self.builder.build_unconditional_branch(arm_bb).unwrap();
                            }
                            Pattern::StructDestructure { .. } => {
                                // Type system guarantees this is the correct struct type
                                self.builder.build_unconditional_branch(arm_bb).unwrap();
                            }
                            _ => {
                                self.builder.build_unconditional_branch(next_bb).unwrap();
                            }
                        }

                        self.builder.position_at_end(arm_bb);

                        // Store bound value into a named local variable
                        if let Pattern::Binding(bind_name) = &arm.pattern {
                            let bind_ty = self.guess_expr_ty(expr, params);
                            let alloca = self.build_alloca(bind_name, &bind_ty);
                            self.builder.build_store(alloca, val).unwrap();
                            self.variables.insert(bind_name.clone(), alloca);
                            self.var_types.insert(bind_name.clone(), bind_ty);
                        }

                        // Struct destructuring: extract and bind fields
                        if let Pattern::StructDestructure { struct_name, fields } = &arm.pattern {
                            let struct_val = val.into_struct_value();
                            for (field_name, sub_pattern) in fields {
                                match sub_pattern {
                                    None => {
                                        // Shorthand: bind field to same-name variable
                                        if let Some((idx, field_ty)) = self.struct_field_info(struct_name, field_name) {
                                            let field_val = self.builder
                                                .build_extract_value(struct_val, idx, &format!("fld_{}", field_name))
                                                .unwrap();
                                            let alloca = self.build_alloca(field_name, &field_ty);
                                            self.builder.build_store(alloca, field_val).unwrap();
                                            self.variables.insert(field_name.clone(), alloca);
                                            self.var_types.insert(field_name.clone(), field_ty);
                                        }
                                    }
                                    Some(sub) => {
                                        // Nested pattern: extract field value, handle sub-pattern
                                        if let Some((idx, field_ty)) = self.struct_field_info(struct_name, field_name) {
                                            let field_val = self.builder
                                                .build_extract_value(struct_val, idx, &format!("fld_{}", field_name))
                                                .unwrap();
                                            match sub.as_ref() {
                                                Pattern::EnumVariant { enum_name: _, variant, binding: Some(bind_name) } => {
                                                    if let Ty::Enum(ename) = &field_ty {
                                                        if let Some(variants) = self.enum_defs.get(ename).cloned() {
                                                            if let Some(v) = variants.iter().find(|v| v.name == *variant) {
                                                                if let Some(ref payload_ty) = v.ty {
                                                                    // Store field value to alloca so we can GEP into it
                                                                    let field_alloca = self.build_alloca(
                                                                        &format!("fld_enum_{}", field_name),
                                                                        &field_ty,
                                                                    );
                                                                    self.builder.build_store(field_alloca, field_val).unwrap();
                                                                    let enum_st = self.enum_types[ename];
                                                                    let payload_ptr = self.builder
                                                                        .build_struct_gep(enum_st, field_alloca, 1, "payload_ptr")
                                                                        .unwrap();
                                                                    let payload_llvm_ty = self.ty_to_basic(payload_ty);
                                                                    let payload_val = self.builder
                                                                        .build_load(payload_llvm_ty, payload_ptr, bind_name)
                                                                        .unwrap();
                                                                    let bind_alloca = self.build_alloca(bind_name, payload_ty);
                                                                    self.builder.build_store(bind_alloca, payload_val).unwrap();
                                                                    self.variables.insert(bind_name.clone(), bind_alloca);
                                                                    self.var_types.insert(bind_name.clone(), payload_ty.clone());
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                Pattern::EnumVariant { enum_name: _, variant, binding: None } => {
                                                    // Emit a discriminant check: if the field's enum tag doesn't
                                                    // match the expected variant, jump to the next arm.
                                                    if let Ty::Enum(ename) = &field_ty {
                                                        if let Some(tags) = self.enum_variant_tags.get(ename).cloned() {
                                                            if let Some(&expected_tag) = tags.get(variant.as_str()) {
                                                                let enum_struct_val = field_val.into_struct_value();
                                                                let actual_tag = self.builder
                                                                    .build_extract_value(enum_struct_val, 0, "disc_tag")
                                                                    .unwrap()
                                                                    .into_int_value();
                                                                let expected = self.context.i32_type().const_int(expected_tag as u64, false);
                                                                let cond = self.builder
                                                                    .build_int_compare(
                                                                        inkwell::IntPredicate::EQ,
                                                                        actual_tag,
                                                                        expected,
                                                                        "disc_cmp",
                                                                    )
                                                                    .unwrap();
                                                                let disc_ok_bb = self.context.append_basic_block(func, "disc_ok");
                                                                self.builder
                                                                    .build_conditional_branch(cond, disc_ok_bb, next_bb)
                                                                    .unwrap();
                                                                self.builder.position_at_end(disc_ok_bb);
                                                            }
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Guard check: if guard fails, skip this arm and go to next_bb
                        if let Some(guard_expr) = &arm.guard {
                            let guard_val = self.compile_expr(guard_expr, params).into_int_value();
                            let guard_pass_bb = self.context.append_basic_block(func, "guard_pass");
                            self.builder
                                .build_conditional_branch(guard_val, guard_pass_bb, next_bb)
                                .unwrap();
                            self.builder.position_at_end(guard_pass_bb);
                        }

                        self.compile_stmts(&arm.body, params);
                        if self.no_terminator() {
                            self.builder.build_unconditional_branch(merge_bb).unwrap();
                        }
                        remaining_bb = next_bb;
                    }

                    if remaining_bb != merge_bb {
                        self.builder.position_at_end(remaining_bb);
                        if self.no_terminator() {
                            self.builder.build_unconditional_branch(merge_bb).unwrap();
                        }
                    }

                    self.builder.position_at_end(merge_bb);
                }
            }

            Stmt::ExprStmt(e) => {
                self.compile_expr(e, params);
            }
            Stmt::ConstDecl { ty, name, value } => {
                // const는 VarDecl과 동일하게 코드젠 (불변성은 정적 분석)
                self.compile_stmt(
                    &Stmt::VarDecl {
                        ty: ty.clone(),
                        name: name.clone(),
                        value: value.clone(),
                    },
                    params,
                );
            }
        }
    }

    pub(super) fn emit_print(&mut self, val: BasicValueEnum<'ctx>, ty: Option<&Ty>) {
        let printf = self.module.get_function("printf").unwrap();
        match val {
            BasicValueEnum::IntValue(iv) => {
                let width = iv.get_type().get_bit_width();
                if width == 1 {
                    // bool(i1)
                    let fmt = self.global_string_ptr("%s\n", "fmt_bool");
                    let true_str = self.global_string_ptr("true", "s_true");
                    let false_str = self.global_string_ptr("false", "s_false");
                    let selected = self
                        .builder
                        .build_select(iv, true_str, false_str, "sel")
                        .unwrap();
                    self.builder
                        .build_call(printf, &[fmt.into(), selected.into()], "")
                        .unwrap();
                } else {
                    // i8~i64, u8~u64 → extend to i64 for printf
                    let print_val = if width < 64 {
                        let is_unsigned = matches!(
                            ty,
                            Some(Ty::U8) | Some(Ty::U16) | Some(Ty::U32) | Some(Ty::U64)
                        );
                        if is_unsigned {
                            self.builder
                                .build_int_z_extend(iv, self.i64_type(), "ext_print")
                                .unwrap()
                        } else {
                            self.builder
                                .build_int_s_extend(iv, self.i64_type(), "ext_print")
                                .unwrap()
                        }
                    } else {
                        iv
                    };
                    let is_unsigned = matches!(
                        ty,
                        Some(Ty::U8) | Some(Ty::U16) | Some(Ty::U32) | Some(Ty::U64)
                    );
                    let fmt_str = if is_unsigned { "%llu\n" } else { "%lld\n" };
                    let fmt = self.global_string_ptr(fmt_str, "fmt_int");
                    self.builder
                        .build_call(printf, &[fmt.into(), print_val.into()], "")
                        .unwrap();
                }
            }
            BasicValueEnum::FloatValue(fv) => {
                let fmt = self.global_string_ptr("%f\n", "fmt_float");
                self.builder
                    .build_call(printf, &[fmt.into(), fv.into()], "")
                    .unwrap();
            }
            BasicValueEnum::PointerValue(pv) => {
                let fmt = self.global_string_ptr("%s\n", "fmt_str");
                self.builder
                    .build_call(printf, &[fmt.into(), pv.into()], "")
                    .unwrap();
            }
            _ => {}
        }
    }
}

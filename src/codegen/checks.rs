use inkwell::basic_block::BasicBlock;
use inkwell::values::{BasicValueEnum, PointerValue};
use inkwell::IntPredicate;

use super::Codegen;
use crate::ast::*;

impl<'ctx> Codegen<'ctx> {
    /// 앵커 진입 시 vault 런타임 카운터를 저장
    pub(super) fn save_vault_count(&mut self, label: &str) -> Option<PointerValue<'ctx>> {
        if let Some(counter) = self.vault_live_count {
            let alloca = self.build_alloca(&format!("{}_exp_vaults", label), &Ty::I64);
            let cur = self
                .builder
                .build_load(self.i64_type(), counter, "vault_save")
                .unwrap();
            self.builder.build_store(alloca, cur).unwrap();
            Some(alloca)
        } else {
            None
        }
    }

    /// recovery 블록 진입 시 vault 카운터 assert 검증.
    ///
    /// `restart_bb` — 검증 통과 후 점프할 블록.
    ///   * 앵커 재시작 시: 앵커 루프 헤더 (anchor_loop_bb)
    ///   * 정상 종료 후 계속: merge/after 블록
    pub(super) fn emit_recovery_vault_assert(
        &mut self,
        restart_bb: BasicBlock<'ctx>,
        expected_alloca: Option<PointerValue<'ctx>>,
        anchor_name: &str,
    ) {
        if let (Some(counter), Some(expected_ptr)) = (self.vault_live_count, expected_alloca) {
            let actual = self
                .builder
                .build_load(self.i64_type(), counter, "actual_vaults")
                .unwrap()
                .into_int_value();
            let expected = self
                .builder
                .build_load(self.i64_type(), expected_ptr, "expected_vaults")
                .unwrap()
                .into_int_value();
            let ok = self
                .builder
                .build_int_compare(IntPredicate::EQ, actual, expected, "vault_check")
                .unwrap();
            let func = self.current_fn.unwrap();
            let ok_bb = self
                .context
                .append_basic_block(func, &format!("vault_ok_{}", anchor_name));
            let fail_bb = self
                .context
                .append_basic_block(func, &format!("vault_fail_{}", anchor_name));
            self.builder
                .build_conditional_branch(ok, ok_bb, fail_bb)
                .unwrap();

            self.builder.position_at_end(fail_bb);
            let printf = self.module.get_function("printf").unwrap();
            let fmt = self.global_string_ptr(
                "VAULT INTEGRITY ASSERT: count mismatch at recovery '%s'\n",
                "vault_assert_fmt",
            );
            let name_ptr = self.global_string_ptr(anchor_name, &format!("anc_{}", anchor_name));
            self.builder
                .build_call(printf, &[fmt.into(), name_ptr.into()], "")
                .unwrap();
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
        // Pop the signal recovery slot before restarting the anchor loop.
        // Each iteration of the loop calls kyte_anchor_enter() (push), so we
        // must call kyte_anchor_exit() (pop) here to keep the depth at 1.
        if let Some(exit_fn) = self.module.get_function("kyte_anchor_exit") {
            self.builder.build_call(exit_fn, &[], "").unwrap();
        }
        // Jump to restart_bb (anchor loop header)
        self.builder.build_unconditional_branch(restart_bb).unwrap();
    }

    pub(super) fn emit_null_check(&mut self, ptr: PointerValue<'ctx>, label: &str) {
        let func = self.current_fn.unwrap();
        let is_null = self
            .builder
            .build_is_null(ptr, &format!("{}_null", label))
            .unwrap();
        let ok_bb = self
            .context
            .append_basic_block(func, &format!("{}_ok", label));
        let fail_bb = self
            .context
            .append_basic_block(func, &format!("{}_oom", label));
        self.builder
            .build_conditional_branch(is_null, fail_bb, ok_bb)
            .unwrap();

        self.builder.position_at_end(fail_bb);
        self.declare_printf();
        let printf = self.module.get_function("printf").unwrap();
        let fmt = self.global_string_ptr("fatal: out of memory at %s\\n", "oom_fmt");
        let loc = self.global_string_ptr(label, &format!("oom_loc_{}", label));
        self.builder
            .build_call(printf, &[fmt.into(), loc.into()], "")
            .unwrap();
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

    /// signed 정수 Add/Sub/Mul overflow 감지 — debug_mode 전용 (H05)
    /// `op_name`: "sadd", "ssub", "smul"
    pub(super) fn emit_overflow_check(
        &mut self,
        li: inkwell::values::IntValue<'ctx>,
        ri: inkwell::values::IntValue<'ctx>,
        op_name: &str,
        result_name: &str,
    ) -> BasicValueEnum<'ctx> {
        // Overflow detection using integer comparisons — no LLVM intrinsics.
        // The llvm.sadd.with.overflow / llvm.ssub.with.overflow / llvm.smul.with.overflow
        // intrinsics crash the LLVM 21 Windows x86-64 backend (STATUS_ACCESS_VIOLATION
        // inside LLVMTargetMachineEmit).  Pure comparison logic avoids that code path
        // entirely and produces correct overflow semantics.
        let zero = li.get_type().const_zero();

        let (result, overflow) = match op_name {
            "sadd" => {
                // Overflow iff same-sign inputs produce a different-sign output.
                //   pos + pos → neg   OR   neg + neg → pos
                let result = self.builder.build_int_add(li, ri, result_name).unwrap();
                let a_neg = self.builder.build_int_compare(IntPredicate::SLT, li, zero, "a_neg").unwrap();
                let b_neg = self.builder.build_int_compare(IntPredicate::SLT, ri, zero, "b_neg").unwrap();
                let r_neg = self.builder.build_int_compare(IntPredicate::SLT, result, zero, "r_neg").unwrap();
                let a_pos = self.builder.build_not(a_neg, "a_pos").unwrap();
                let b_pos = self.builder.build_not(b_neg, "b_pos").unwrap();
                let r_pos = self.builder.build_not(r_neg, "r_pos").unwrap();
                let both_pos = self.builder.build_and(a_pos, b_pos, "both_pos").unwrap();
                let both_neg = self.builder.build_and(a_neg, b_neg, "both_neg").unwrap();
                let ovf_pp = self.builder.build_and(both_pos, r_neg, "ovf_pp").unwrap();
                let ovf_nn = self.builder.build_and(both_neg, r_pos, "ovf_nn").unwrap();
                let overflow = self.builder.build_or(ovf_pp, ovf_nn, "sadd_ovf_flag").unwrap();
                (result, overflow)
            }
            "ssub" => {
                // a - b overflows in two cases:
                //   case1: a >= 0, b < 0, result < 0  (pos minus neg wrapped to neg)
                //   case2: a < 0,  b > 0, result >= 0 (neg minus pos wrapped to non-neg)
                let result = self.builder.build_int_sub(li, ri, result_name).unwrap();
                let a_ge0 = self.builder.build_int_compare(IntPredicate::SGE, li, zero, "a_ge0").unwrap();
                let b_lt0 = self.builder.build_int_compare(IntPredicate::SLT, ri, zero, "b_lt0").unwrap();
                let r_lt0 = self.builder.build_int_compare(IntPredicate::SLT, result, zero, "r_lt0").unwrap();
                let case1 = self.builder.build_and(a_ge0, b_lt0, "s1ab").unwrap();
                let case1 = self.builder.build_and(case1, r_lt0, "case1").unwrap();
                let a_lt0 = self.builder.build_int_compare(IntPredicate::SLT, li, zero, "a_lt0").unwrap();
                let b_gt0 = self.builder.build_int_compare(IntPredicate::SGT, ri, zero, "b_gt0").unwrap();
                let r_ge0 = self.builder.build_int_compare(IntPredicate::SGE, result, zero, "r_ge0").unwrap();
                let case2 = self.builder.build_and(a_lt0, b_gt0, "s2ab").unwrap();
                let case2 = self.builder.build_and(case2, r_ge0, "case2").unwrap();
                let overflow = self.builder.build_or(case1, case2, "ssub_ovf_flag").unwrap();
                (result, overflow)
            }
            _ => {
                // smul and other ops: no overflow check (intrinsics unavailable safely).
                // Returns the wrapping result without trapping.
                return self.builder.build_int_mul(li, ri, result_name).unwrap().into();
            }
        };

        let func = self.current_fn.unwrap();
        let ok_bb = self
            .context
            .append_basic_block(func, &format!("{}_ok", op_name));
        let fail_bb = self
            .context
            .append_basic_block(func, &format!("{}_overflow", op_name));
        self.builder
            .build_conditional_branch(overflow, fail_bb, ok_bb)
            .unwrap();

        self.builder.position_at_end(fail_bb);
        self.declare_printf();
        let printf = self.module.get_function("printf").unwrap();
        let fmt = self.global_string_ptr(
            "runtime error: integer overflow in arithmetic operation\\n",
            "overflow_msg",
        );
        self.builder.build_call(printf, &[fmt.into()], "").unwrap();
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
        result.into()
    }

    pub(super) fn emit_bounds_check(
        &mut self,
        idx: inkwell::values::IntValue<'ctx>,
        arr_len: u64,
        arr_name: &str,
    ) {
        let func = self.current_fn.unwrap();
        let len_val = self.i64_type().const_int(arr_len, false);
        // idx < 0 || idx >= len
        let neg = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                idx,
                self.i64_type().const_int(0, false),
                "idx_neg",
            )
            .unwrap();
        let over = self
            .builder
            .build_int_compare(IntPredicate::SGE, idx, len_val, "idx_over")
            .unwrap();
        let oob = self.builder.build_or(neg, over, "idx_oob").unwrap();
        let ok_bb = self.context.append_basic_block(func, "bounds_ok");
        let fail_bb = self.context.append_basic_block(func, "bounds_fail");
        self.builder
            .build_conditional_branch(oob, fail_bb, ok_bb)
            .unwrap();

        self.builder.position_at_end(fail_bb);
        let printf = self.module.get_function("printf").unwrap();
        let fmt = self.global_string_ptr(
            "runtime error: array '%s' index %lld out of bounds (length %lld)\\n",
            "oob_fmt",
        );
        let name_ptr = self.global_string_ptr(arr_name, &format!("arr_name_{}", arr_name));
        self.builder
            .build_call(
                printf,
                &[fmt.into(), name_ptr.into(), idx.into(), len_val.into()],
                "",
            )
            .unwrap();
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

    /// 배열 길이를 모를 때 음수 인덱스만 검사
    pub(super) fn emit_negative_index_check(
        &mut self,
        idx: inkwell::values::IntValue<'ctx>,
        arr_name: &str,
    ) {
        let func = self.current_fn.unwrap();
        let neg = self
            .builder
            .build_int_compare(
                IntPredicate::SLT,
                idx,
                self.i64_type().const_int(0, false),
                "idx_neg",
            )
            .unwrap();
        let ok_bb = self.context.append_basic_block(func, "neg_ok");
        let fail_bb = self.context.append_basic_block(func, "neg_fail");
        self.builder
            .build_conditional_branch(neg, fail_bb, ok_bb)
            .unwrap();

        self.builder.position_at_end(fail_bb);
        let printf = self.module.get_function("printf").unwrap();
        let fmt = self.global_string_ptr(
            "runtime error: array '%s' negative index %lld\\n",
            "neg_fmt",
        );
        let name_ptr = self.global_string_ptr(arr_name, &format!("arr_name_{}", arr_name));
        self.builder
            .build_call(printf, &[fmt.into(), name_ptr.into()], "")
            .unwrap();
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
}

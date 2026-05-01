use crate::ast::*;
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, PointerValue};
use inkwell::{FloatPredicate, IntPredicate};

use super::Codegen;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn compile_binop(
        &mut self,
        op: &BinOpKind,
        l: BasicValueEnum<'ctx>,
        r: BasicValueEnum<'ctx>,
        ty: &Ty,
    ) -> BasicValueEnum<'ctx> {
        if Self::is_integer_ty(ty) || matches!(ty, Ty::Array(_)) {
            let li = l.into_int_value();
            let ri = r.into_int_value();
            let signed = Self::is_signed(ty);
            match op {
                BinOpKind::Add => {
                    if signed && self.debug_mode {
                        self.emit_overflow_check(li, ri, "sadd", "add")
                    } else {
                        self.builder.build_int_add(li, ri, "add").unwrap().into()
                    }
                }
                BinOpKind::Sub => {
                    if signed && self.debug_mode {
                        self.emit_overflow_check(li, ri, "ssub", "sub")
                    } else {
                        self.builder.build_int_sub(li, ri, "sub").unwrap().into()
                    }
                }
                BinOpKind::Mul => {
                    if signed && self.debug_mode {
                        self.emit_overflow_check(li, ri, "smul", "mul")
                    } else {
                        self.builder.build_int_mul(li, ri, "mul").unwrap().into()
                    }
                }
                BinOpKind::Div => {
                    let is_zero = self
                        .builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            ri,
                            ri.get_type().const_zero(),
                            "div_zero",
                        )
                        .unwrap();
                    let func = self.current_fn.unwrap();
                    let ok_bb = self.context.append_basic_block(func, "div_ok");
                    let err_bb = self.context.append_basic_block(func, "div_err");
                    self.builder
                        .build_conditional_branch(is_zero, err_bb, ok_bb)
                        .unwrap();

                    self.builder.position_at_end(err_bb);
                    let printf = self.module.get_function("printf").unwrap();
                    let fmt = self
                        .global_string_ptr("runtime error: division by zero\\n", "div_zero_msg");
                    self.builder.build_call(printf, &[fmt.into()], "").unwrap();
                    if let Some(recovery_bb) = self.recovery_stack.last().copied() {
                        self.builder
                            .build_unconditional_branch(recovery_bb)
                            .unwrap();
                    } else {
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

                    self.builder.position_at_end(ok_bb);
                    if signed {
                        self.builder
                            .build_int_signed_div(li, ri, "sdiv")
                            .unwrap()
                            .into()
                    } else {
                        self.builder
                            .build_int_unsigned_div(li, ri, "udiv")
                            .unwrap()
                            .into()
                    }
                }
                BinOpKind::Mod => {
                    let is_zero = self
                        .builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            ri,
                            ri.get_type().const_zero(),
                            "mod_zero",
                        )
                        .unwrap();
                    let func = self.current_fn.unwrap();
                    let ok_bb = self.context.append_basic_block(func, "mod_ok");
                    let err_bb = self.context.append_basic_block(func, "mod_err");
                    self.builder
                        .build_conditional_branch(is_zero, err_bb, ok_bb)
                        .unwrap();

                    self.builder.position_at_end(err_bb);
                    let printf = self.module.get_function("printf").unwrap();
                    let fmt =
                        self.global_string_ptr("runtime error: modulo by zero\\n", "mod_zero_msg");
                    self.builder.build_call(printf, &[fmt.into()], "").unwrap();
                    if let Some(recovery_bb) = self.recovery_stack.last().copied() {
                        self.builder
                            .build_unconditional_branch(recovery_bb)
                            .unwrap();
                    } else {
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

                    self.builder.position_at_end(ok_bb);
                    if signed {
                        self.builder
                            .build_int_signed_rem(li, ri, "srem")
                            .unwrap()
                            .into()
                    } else {
                        self.builder
                            .build_int_unsigned_rem(li, ri, "urem")
                            .unwrap()
                            .into()
                    }
                }
                BinOpKind::Lt => {
                    let pred = if signed {
                        IntPredicate::SLT
                    } else {
                        IntPredicate::ULT
                    };
                    self.builder
                        .build_int_compare(pred, li, ri, "lt")
                        .unwrap()
                        .into()
                }
                BinOpKind::Gt => {
                    let pred = if signed {
                        IntPredicate::SGT
                    } else {
                        IntPredicate::UGT
                    };
                    self.builder
                        .build_int_compare(pred, li, ri, "gt")
                        .unwrap()
                        .into()
                }
                BinOpKind::Le => {
                    let pred = if signed {
                        IntPredicate::SLE
                    } else {
                        IntPredicate::ULE
                    };
                    self.builder
                        .build_int_compare(pred, li, ri, "le")
                        .unwrap()
                        .into()
                }
                BinOpKind::Ge => {
                    let pred = if signed {
                        IntPredicate::SGE
                    } else {
                        IntPredicate::UGE
                    };
                    self.builder
                        .build_int_compare(pred, li, ri, "ge")
                        .unwrap()
                        .into()
                }
                BinOpKind::Eq => self
                    .builder
                    .build_int_compare(IntPredicate::EQ, li, ri, "eq")
                    .unwrap()
                    .into(),
                BinOpKind::Neq => self
                    .builder
                    .build_int_compare(IntPredicate::NE, li, ri, "ne")
                    .unwrap()
                    .into(),
                BinOpKind::And => self.builder.build_and(li, ri, "and").unwrap().into(),
                BinOpKind::Or => self.builder.build_or(li, ri, "or").unwrap().into(),
            }
        } else {
            match ty {
                Ty::Float => {
                    let lf = l.into_float_value();
                    let rf = r.into_float_value();
                    match op {
                        BinOpKind::Add => {
                            self.builder.build_float_add(lf, rf, "fadd").unwrap().into()
                        }
                        BinOpKind::Sub => {
                            self.builder.build_float_sub(lf, rf, "fsub").unwrap().into()
                        }
                        BinOpKind::Mul => {
                            self.builder.build_float_mul(lf, rf, "fmul").unwrap().into()
                        }
                        BinOpKind::Div => {
                            self.builder.build_float_div(lf, rf, "fdiv").unwrap().into()
                        }
                        BinOpKind::Mod => {
                            self.builder.build_float_rem(lf, rf, "fmod").unwrap().into()
                        }
                        BinOpKind::Lt => self
                            .builder
                            .build_float_compare(FloatPredicate::OLT, lf, rf, "flt")
                            .unwrap()
                            .into(),
                        BinOpKind::Gt => self
                            .builder
                            .build_float_compare(FloatPredicate::OGT, lf, rf, "fgt")
                            .unwrap()
                            .into(),
                        BinOpKind::Le => self
                            .builder
                            .build_float_compare(FloatPredicate::OLE, lf, rf, "fle")
                            .unwrap()
                            .into(),
                        BinOpKind::Ge => self
                            .builder
                            .build_float_compare(FloatPredicate::OGE, lf, rf, "fge")
                            .unwrap()
                            .into(),
                        BinOpKind::Eq => self
                            .builder
                            .build_float_compare(FloatPredicate::OEQ, lf, rf, "feq")
                            .unwrap()
                            .into(),
                        BinOpKind::Neq => self
                            .builder
                            .build_float_compare(FloatPredicate::ONE, lf, rf, "fne")
                            .unwrap()
                            .into(),
                        _ => l,
                    }
                }
                Ty::Bool => {
                    let li = l.into_int_value();
                    let ri = r.into_int_value();
                    match op {
                        BinOpKind::And => self.builder.build_and(li, ri, "band").unwrap().into(),
                        BinOpKind::Or => self.builder.build_or(li, ri, "bor").unwrap().into(),
                        BinOpKind::Eq => self
                            .builder
                            .build_int_compare(IntPredicate::EQ, li, ri, "beq")
                            .unwrap()
                            .into(),
                        BinOpKind::Neq => self
                            .builder
                            .build_int_compare(IntPredicate::NE, li, ri, "bne")
                            .unwrap()
                            .into(),
                        _ => l,
                    }
                }
                Ty::String => {
                    let lp = l.into_pointer_value();
                    let rp = r.into_pointer_value();
                    let strcmp = self.module.get_function("strcmp").unwrap();
                    let cmp_result = self
                        .builder
                        .build_call(strcmp, &[lp.into(), rp.into()], "strcmp_ret")
                        .unwrap()
                        .try_as_basic_value()
                        .basic()
                        .unwrap()
                        .into_int_value();
                    let zero = self.context.i32_type().const_int(0, false);
                    match op {
                        BinOpKind::Eq => self
                            .builder
                            .build_int_compare(IntPredicate::EQ, cmp_result, zero, "str_eq")
                            .unwrap()
                            .into(),
                        BinOpKind::Neq => self
                            .builder
                            .build_int_compare(IntPredicate::NE, cmp_result, zero, "str_ne")
                            .unwrap()
                            .into(),
                        BinOpKind::Lt => self
                            .builder
                            .build_int_compare(IntPredicate::SLT, cmp_result, zero, "str_lt")
                            .unwrap()
                            .into(),
                        BinOpKind::Gt => self
                            .builder
                            .build_int_compare(IntPredicate::SGT, cmp_result, zero, "str_gt")
                            .unwrap()
                            .into(),
                        BinOpKind::Le => self
                            .builder
                            .build_int_compare(IntPredicate::SLE, cmp_result, zero, "str_le")
                            .unwrap()
                            .into(),
                        BinOpKind::Ge => self
                            .builder
                            .build_int_compare(IntPredicate::SGE, cmp_result, zero, "str_ge")
                            .unwrap()
                            .into(),
                        _ => l,
                    }
                }
                _ => l,
            }
        }
    }

    pub(super) fn build_str_concat(
        &mut self,
        l: BasicValueEnum<'ctx>,
        r: BasicValueEnum<'ctx>,
        lty: &Ty,
        rty: &Ty,
        _params: &[Param],
    ) -> BasicValueEnum<'ctx> {
        let strlen = self.module.get_function("strlen").unwrap();
        let malloc = self.module.get_function("malloc").unwrap();
        let snprintf = self.module.get_function("snprintf").unwrap();

        let lp = self.to_string_ptr(l, lty);
        let rp = self.to_string_ptr(r, rty);

        let len_l = self
            .builder
            .build_call(strlen, &[lp.into()], "len_l")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let len_r = self
            .builder
            .build_call(strlen, &[rp.into()], "len_r")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let total = self
            .builder
            .build_int_add(len_l, len_r, "total_len")
            .unwrap();
        let total_plus1 = self
            .builder
            .build_int_add(total, self.i64_type().const_int(1, false), "buf_size")
            .unwrap();

        let buf = self
            .builder
            .build_call(malloc, &[total_plus1.into()], "concat_buf")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        self.emit_null_check(buf, "string_concat");

        // Do NOT track in string_temp_stack.  Strings may be assigned to
        // variables or returned from functions, so eagerly freeing them at
        // scope exit causes use-after-free.  We accept the corresponding
        // memory leak until a proper ownership/RC system is added.

        let fmt = self.global_string_ptr("%s%s", "concat_fmt");
        self.builder
            .build_call(
                snprintf,
                &[
                    buf.into(),
                    total_plus1.into(),
                    fmt.into(),
                    lp.into(),
                    rp.into(),
                ],
                "",
            )
            .unwrap();

        buf.into()
    }

    #[allow(clippy::wrong_self_convention)]
    pub(super) fn to_string_ptr(
        &mut self,
        val: BasicValueEnum<'ctx>,
        ty: &Ty,
    ) -> PointerValue<'ctx> {
        if *ty == Ty::String {
            return val.into_pointer_value();
        }

        // Bool: return global literal — zero allocation
        if *ty == Ty::Bool {
            let true_str = self.global_string_ptr("true", "s_true");
            let false_str = self.global_string_ptr("false", "s_false");
            return self
                .builder
                .build_select(val.into_int_value(), true_str, false_str, "bsel")
                .unwrap()
                .into_pointer_value();
        }

        // Numeric: fixed 32-byte stack buffer — no malloc, single snprintf.
        // Max length: i64 = 20 chars, f64 = ~25 chars — 32 is always sufficient.
        // Alloca is hoisted to the function entry block so it does not grow the
        // stack on each loop iteration when string-concat is used inside a loop.
        let snprintf = self.module.get_function("snprintf").unwrap();
        let i8_ty = self.context.i8_type();
        let buf_size = 32u64;
        let buf = {
            let entry_bb = self.current_fn.unwrap().get_first_basic_block().unwrap();
            let cur_bb = self.builder.get_insert_block();
            match entry_bb.get_first_instruction() {
                Some(inst) => self.builder.position_before(&inst),
                None => self.builder.position_at_end(entry_bb),
            }
            let b = self
                .builder
                .build_array_alloca(i8_ty, self.i64_type().const_int(buf_size, false), "num_buf")
                .unwrap();
            if let Some(bb) = cur_bb {
                self.builder.position_at_end(bb);
            }
            b
        };

        let (fmt_ptr, arg): (PointerValue<'ctx>, BasicMetadataValueEnum<'ctx>) = match ty {
            Ty::Float => {
                let fmt = self.global_string_ptr("%f", "fmt_f2s");
                (fmt, val.into())
            }
            _ => {
                let iv = val.into_int_value();
                let is_unsigned = matches!(ty, Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64);
                let wide = if iv.get_type().get_bit_width() < 64 {
                    if is_unsigned {
                        self.builder
                            .build_int_z_extend(iv, self.i64_type(), "ext")
                            .unwrap()
                    } else {
                        self.builder
                            .build_int_s_extend(iv, self.i64_type(), "ext")
                            .unwrap()
                    }
                } else {
                    iv
                };
                let fmt =
                    self.global_string_ptr(if is_unsigned { "%llu" } else { "%lld" }, "fmt_i2s");
                (fmt, wide.into())
            }
        };

        self.builder
            .build_call(
                snprintf,
                &[
                    buf.into(),
                    self.i64_type().const_int(buf_size, false).into(),
                    fmt_ptr.into(),
                    arg,
                ],
                "",
            )
            .unwrap();

        buf
    }

    pub(super) fn build_cast(
        &self,
        val: BasicValueEnum<'ctx>,
        src: &Ty,
        dst: &Ty,
    ) -> BasicValueEnum<'ctx> {
        match (src, dst) {
            (s, d) if s == d => val,
            (s, Ty::Float) if Self::is_integer_ty(s) => {
                let iv = val.into_int_value();
                if Self::is_signed(s) {
                    self.builder
                        .build_signed_int_to_float(iv, self.f64_type(), "si2f")
                        .unwrap()
                        .into()
                } else {
                    self.builder
                        .build_unsigned_int_to_float(iv, self.f64_type(), "ui2f")
                        .unwrap()
                        .into()
                }
            }
            (Ty::Float, d) if Self::is_integer_ty(d) => {
                let fv = val.into_float_value();
                let target_ty = self.ty_to_int_type(d);
                if Self::is_signed(d) {
                    self.builder
                        .build_float_to_signed_int(fv, target_ty, "f2si")
                        .unwrap()
                        .into()
                } else {
                    self.builder
                        .build_float_to_unsigned_int(fv, target_ty, "f2ui")
                        .unwrap()
                        .into()
                }
            }
            (s, d) if Self::is_integer_ty(s) && Self::is_integer_ty(d) => self.coerce_to_ty(val, d),
            (Ty::Bool, d) if Self::is_integer_ty(d) => {
                let iv = val.into_int_value();
                let target_ty = self.ty_to_int_type(d);
                self.builder
                    .build_int_z_extend(iv, target_ty, "b2i")
                    .unwrap()
                    .into()
            }
            (s, Ty::Bool) if Self::is_integer_ty(s) => {
                let iv = val.into_int_value();
                let zero = iv.get_type().const_int(0, false);
                self.builder
                    .build_int_compare(IntPredicate::NE, iv, zero, "i2b")
                    .unwrap()
                    .into()
            }
            _ => val,
        }
    }
}

use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum};

use super::Codegen;
use crate::ast::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn compile_expr(&mut self, expr: &Expr, params: &[Param]) -> BasicValueEnum<'ctx> {
        match expr {
            Expr::IntLit(n) => self.i64_type().const_int(*n as u64, true).into(),
            Expr::FloatLit(f) => self.f64_type().const_float(*f).into(),
            Expr::StringLit(s) => self.global_string_ptr(s, "str").into(),
            Expr::Bool(b) => self
                .bool_type()
                .const_int(if *b { 1 } else { 0 }, false)
                .into(),
            Expr::Ident(name, _) => {
                let ty = self.guess_var_ty(name, params);
                self.load_var(name, &ty)
            }
            Expr::UnaryOp { op, expr } => {
                let val = self.compile_expr(expr, params);
                match op {
                    UnaryOpKind::Neg => match val {
                        BasicValueEnum::IntValue(iv) => {
                            self.builder.build_int_neg(iv, "neg").unwrap().into()
                        }
                        BasicValueEnum::FloatValue(fv) => {
                            self.builder.build_float_neg(fv, "fneg").unwrap().into()
                        }
                        _ => val,
                    },
                    UnaryOpKind::Not => {
                        let iv = val.into_int_value();
                        self.builder.build_not(iv, "not").unwrap().into()
                    }
                }
            }
            Expr::BinOp { left, op, right } => {
                let left_ty = self.guess_expr_ty(left, params);
                let right_ty = self.guess_expr_ty(right, params);

                // string + string → concat
                if matches!(op, BinOpKind::Add) && (left_ty == Ty::String || right_ty == Ty::String)
                {
                    let l = self.compile_expr(left, params);
                    let r = self.compile_expr(right, params);
                    return self.build_str_concat(l, r, &left_ty, &right_ty, params);
                }

                let mut l = self.compile_expr(left, params);
                let mut r = self.compile_expr(right, params);
                let ty = left_ty;
                if Self::is_integer_ty(&ty) {
                    l = self.coerce_to_ty(l, &ty);
                    r = self.coerce_to_ty(r, &ty);
                }
                self.compile_binop(op, l, r, &ty)
            }
            Expr::Call { name, args, .. } => {
                // len() 빌트인
                if name == "len" {
                    if let Expr::Ident(arg_name, _) = &args[0] {
                        let len = self
                            .array_lengths
                            .get(arg_name.as_str())
                            .copied()
                            .unwrap_or(0);
                        return self.i64_type().const_int(len, false).into();
                    }
                    return self.i64_type().const_int(0, false).into();
                }

                // emit() — synchronous event dispatch
                if name == "emit" {
                    let emit_fn = self.module.get_function("kyte_emit_event").unwrap();
                    let event_name_val = self.compile_expr(&args[0], params);
                    let payload_ptr = if args.len() > 1 {
                        let pv = self.compile_expr(&args[1], params);
                        match pv {
                            BasicValueEnum::PointerValue(p) => p,
                            _ => self.global_string_ptr("", "emit_empty_payload"),
                        }
                    } else {
                        self.global_string_ptr("", "emit_empty_payload")
                    };
                    self.builder
                        .build_call(
                            emit_fn,
                            &[event_name_val.into(), payload_ptr.into()],
                            "",
                        )
                        .unwrap();
                    return self.i64_type().const_int(0, false).into();
                }

                // 함수 포인터 변수로 간주 (클로저 호출)
                if !self.functions.contains_key(name) {
                    if let Some(&fn_ptr_alloca) = self.variables.get(name) {
                        let fn_ptr = self
                            .builder
                            .build_load(self.ptr_type(), fn_ptr_alloca, name)
                            .unwrap()
                            .into_pointer_value();
                        let compiled_args: Vec<BasicMetadataValueEnum> = args
                            .iter()
                            .map(|a| self.compile_expr(a, params).into())
                            .collect();
                        let arg_types: Vec<BasicMetadataTypeEnum> = compiled_args
                            .iter()
                            .map(|a| match a {
                                BasicMetadataValueEnum::IntValue(v) => v.get_type().into(),
                                BasicMetadataValueEnum::PointerValue(v) => v.get_type().into(),
                                BasicMetadataValueEnum::FloatValue(v) => v.get_type().into(),
                                _ => self.i64_type().into(),
                            })
                            .collect();
                        let fn_type = self.i64_type().fn_type(&arg_types, false);
                        let call_site = self
                            .builder
                            .build_indirect_call(fn_type, fn_ptr, &compiled_args, "closure_ret")
                            .unwrap();
                        return call_site
                            .try_as_basic_value()
                            .basic()
                            .unwrap_or_else(|| self.i64_type().const_int(0, false).into());
                    }
                    return self.i64_type().const_int(0, false).into();
                }

                let func = self.functions[name];
                let compiled_args: Vec<BasicMetadataValueEnum> = args
                    .iter()
                    .map(|a| self.compile_expr(a, params).into())
                    .collect();
                let call_site = self
                    .builder
                    .build_call(func, &compiled_args, "ret")
                    .unwrap();
                call_site
                    .try_as_basic_value()
                    .basic()
                    .unwrap_or_else(|| self.i64_type().const_int(0, false).into())
            }
            Expr::MethodCall { base, method, args } => {
                // mod.func() 호출: base가 모듈 이름인 경우
                if let Expr::Ident(base_name, _) = base.as_ref() {
                    if self.module_names.contains(base_name.as_str()) {
                        let qualified = format!("{}_{}", base_name, method);
                        if let Some(&func) = self.functions.get(&qualified) {
                            let compiled_args: Vec<BasicMetadataValueEnum> = args
                                .iter()
                                .map(|a| self.compile_expr(a, params).into())
                                .collect();
                            let call_site = self
                                .builder
                                .build_call(func, &compiled_args, "ret")
                                .unwrap();
                            return call_site
                                .try_as_basic_value()
                                .basic()
                                .unwrap_or_else(|| self.i64_type().const_int(0, false).into());
                        }
                    }
                }
                if let Ty::Struct(sname) = self.guess_expr_ty(base, params) {
                    let fn_name = format!("{}.{}", sname, method);
                    if let Some(func) = self.functions.get(&fn_name).copied() {
                        let mut compiled_args: Vec<BasicMetadataValueEnum> = Vec::new();
                        compiled_args.push(self.compile_expr(base, params).into());
                        for a in args {
                            compiled_args.push(self.compile_expr(a, params).into());
                        }
                        let call_site = self
                            .builder
                            .build_call(func, &compiled_args, "ret")
                            .unwrap();
                        return call_site
                            .try_as_basic_value()
                            .basic()
                            .unwrap_or_else(|| self.i64_type().const_int(0, false).into());
                    }
                }
                self.i64_type().const_int(0, false).into()
            }

            Expr::ArrayLit(elems) => {
                let elem_ty = self.guess_expr_ty(&elems[0], params);
                let elem_llvm_ty = self.elem_llvm_type(&elem_ty);
                let count = elems.len() as u64;
                let size = self.i64_type().const_int(count, false);
                let data_ptr = self
                    .builder
                    .build_array_alloca(elem_llvm_ty, size, "arr_data")
                    .unwrap();
                for (i, elem) in elems.iter().enumerate() {
                    let val = self.compile_expr(elem, params);
                    let idx = self.i64_type().const_int(i as u64, false);
                    let gep = unsafe {
                        self.builder
                            .build_gep(elem_llvm_ty, data_ptr, &[idx], "arr_elem")
                            .unwrap()
                    };
                    self.builder.build_store(gep, val).unwrap();
                }
                data_ptr.into()
            }

            Expr::Index { array, index } => {
                let arr_ty = self.guess_expr_ty(array, params);
                let inner = match &arr_ty {
                    Ty::Array(inner) => *inner.clone(),
                    _ => Ty::Int,
                };
                // 런타임 배열 범위 검사 (C04)
                let arr_name = if let Expr::Ident(n, _) = array.as_ref() {
                    Some(n.as_str())
                } else {
                    None
                };
                let data_ptr = self.compile_expr(array, params).into_pointer_value();
                let idx = self.compile_expr(index, params).into_int_value();
                if let Some(n) = arr_name {
                    if let Some(&arr_len) = self.array_lengths.get(n) {
                        self.emit_bounds_check(idx, arr_len, n);
                    } else {
                        self.emit_negative_index_check(idx, n);
                    }
                } else {
                    self.emit_negative_index_check(idx, "<expr>");
                }
                let elem_llvm_ty = self.elem_llvm_type(&inner);
                let gep = unsafe {
                    self.builder
                        .build_gep(elem_llvm_ty, data_ptr, &[idx], "idx_ptr")
                        .unwrap()
                };
                self.builder
                    .build_load(elem_llvm_ty, gep, "idx_val")
                    .unwrap()
            }

            Expr::Cast {
                expr,
                ty: target_ty,
            } => {
                let val = self.compile_expr(expr, params);
                let src_ty = self.guess_expr_ty(expr, params);
                self.build_cast(val, &src_ty, target_ty)
            }
            Expr::StructInit { name, fields } => {
                let st = self.struct_types[name];
                let mut agg = st.get_undef();
                // Collect only (name, ty) pairs to avoid cloning StructField and release borrow
                let field_info: Vec<(String, Ty)> = self
                    .struct_defs
                    .get(name)
                    .map(|defs| {
                        defs.iter()
                            .map(|f| (f.name.clone(), f.ty.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                for (idx, (fname, fty)) in field_info.iter().enumerate() {
                    let value = if let Some((_, expr)) = fields.iter().find(|(n, _)| n == fname) {
                        let v = self.compile_expr(expr, params);
                        self.coerce_to_ty(v, fty)
                    } else {
                        self.ty_to_basic(fty).const_zero()
                    };
                    agg = self
                        .builder
                        .build_insert_value(agg, value, idx as u32, "sset")
                        .unwrap()
                        .into_struct_value();
                }
                agg.into()
            }
            Expr::FieldAccess { base, field } => {
                let bt = self.guess_expr_ty(base, params);
                if let Ty::Struct(sname) = bt {
                    if let Some((idx, field_ty)) = self.struct_field_info(&sname, field) {
                        let struct_val = self.compile_expr(base, params).into_struct_value();
                        return self
                            .builder
                            .build_extract_value(struct_val, idx, "field_get")
                            .unwrap_or_else(|_| self.ty_to_basic(&field_ty).const_zero());
                    }
                }
                self.i64_type().const_int(0, false).into()
            }

            Expr::EnumVariant {
                enum_name,
                variant,
                value,
            } => {
                if let Some(st) = self.enum_types.get(enum_name).copied() {
                    let mut agg = st.get_undef();
                    let tag = self
                        .enum_variant_tags
                        .get(enum_name)
                        .and_then(|m| m.get(variant))
                        .copied()
                        .unwrap_or(0);
                    let tag_val = self.context.i32_type().const_int(tag as u64, false);
                    agg = self
                        .builder
                        .build_insert_value(agg, tag_val, 0, "set_tag")
                        .unwrap()
                        .into_struct_value();
                    // If the variant has a payload, store it into field 1
                    if let Some(val_expr) = value {
                        let payload = self.compile_expr(val_expr, params);
                        // Alloca the enum struct, store tag, then bitcast field 1 to payload type and store
                        let alloca = self.build_alloca("enum_tmp", &Ty::Enum(enum_name.clone()));
                        self.builder.build_store(alloca, agg).unwrap();
                        let payload_ptr = self
                            .builder
                            .build_struct_gep(st, alloca, 1, "payload_ptr")
                            .unwrap();
                        self.builder.build_store(payload_ptr, payload).unwrap();
                        return self
                            .builder
                            .build_load(st.as_basic_type_enum(), alloca, "enum_val")
                            .unwrap();
                    }
                    agg.into()
                } else {
                    self.i64_type().const_int(0, false).into()
                }
            }
            Expr::Closure {
                params: cl_params,
                body,
            } => {
                // 클로저를 실제 LLVM 함수로 emit (캡처 없는 함수 포인터)
                let id = self.closure_counter;
                self.closure_counter += 1;
                let cl_name = format!("__closure_{}", id);

                // 파라미터 타입 결정
                let param_tys: Vec<BasicMetadataTypeEnum> = cl_params
                    .iter()
                    .map(|(_, opt_ty)| {
                        let ty = opt_ty.as_ref().cloned().unwrap_or(Ty::Int);
                        self.ty_to_basic(&ty).into()
                    })
                    .collect();
                let fn_type = self.i64_type().fn_type(&param_tys, false);
                let cl_fn = self.module.add_function(&cl_name, fn_type, None);
                self.functions.insert(cl_name.clone(), cl_fn);
                self.fn_return_tys.insert(cl_name.clone(), Some(Ty::Int));

                // 현재 상태 저장 (clone 없이 swap)
                let saved_fn = self.current_fn;
                let saved_bb = self.builder.get_insert_block();
                let mut saved_vars = std::mem::take(&mut self.variables);
                let mut saved_var_types = std::mem::take(&mut self.var_types);

                // 클로저 함수 본문 빌드
                let entry_bb = self.context.append_basic_block(cl_fn, "entry");
                self.builder.position_at_end(entry_bb);
                self.current_fn = Some(cl_fn);

                for (i, (param_name, opt_ty)) in cl_params.iter().enumerate() {
                    let ty = opt_ty.as_ref().cloned().unwrap_or(Ty::Int);
                    let alloca = self.build_alloca(param_name, &ty);
                    let pval = cl_fn.get_nth_param(i as u32).unwrap();
                    self.builder.build_store(alloca, pval).unwrap();
                    self.variables.insert(param_name.clone(), alloca);
                    self.var_types.insert(param_name.clone(), ty);
                }

                self.compile_stmts(body, &[]);

                if self.no_terminator() {
                    self.builder
                        .build_return(Some(&self.i64_type().const_int(0, false)))
                        .unwrap();
                }

                // 상태 복원 (swap — drop 없이 재사용)
                self.current_fn = saved_fn;
                std::mem::swap(&mut self.variables, &mut saved_vars);
                std::mem::swap(&mut self.var_types, &mut saved_var_types);
                if let Some(bb) = saved_bb {
                    self.builder.position_at_end(bb);
                }

                // 함수 포인터 반환
                cl_fn.as_global_value().as_pointer_value().into()
            }
            Expr::FStringLit(parts) => {
                // f-string: snprintf를 이용해 각 파트를 버퍼에 누적
                let buf_size = 4096u64;
                // i8 타입으로 배열 alloca (포인터로 바로 사용)
                let i8_ty = self.context.i8_type();
                let alloca = self
                    .builder
                    .build_array_alloca(
                        i8_ty,
                        self.i64_type().const_int(buf_size, false),
                        "fstr_buf",
                    )
                    .unwrap();
                // buf[0] = '\0'
                let zero_i8 = i8_ty.const_zero();
                self.builder.build_store(alloca, zero_i8).unwrap();

                let strcat_fn = if let Some(f) = self.module.get_function("strcat") {
                    f
                } else {
                    let i8_ptr = self.ptr_type();
                    let fn_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                    self.module.add_function("strcat", fn_ty, None)
                };
                let snprintf_fn = if let Some(f) = self.module.get_function("snprintf") {
                    f
                } else {
                    let i8_ptr = self.ptr_type();
                    let i64_ty = self.i64_type();
                    let fn_ty =
                        i64_ty.fn_type(&[i8_ptr.into(), i64_ty.into(), i8_ptr.into()], true);
                    self.module.add_function("snprintf", fn_ty, None)
                };

                for part in parts {
                    match part {
                        crate::ast::FStringPart::Literal(s) => {
                            let lit_ptr = self.global_string_ptr(s, "fstr_lit");
                            self.builder
                                .build_call(strcat_fn, &[alloca.into(), lit_ptr.into()], "")
                                .unwrap();
                        }
                        crate::ast::FStringPart::Expr(e) => {
                            let val = self.compile_expr(e, params);
                            let expr_ty = self.guess_expr_ty(e, params);
                            let tmp = self
                                .builder
                                .build_array_alloca(
                                    i8_ty,
                                    self.i64_type().const_int(64, false),
                                    "fstr_tmp",
                                )
                                .unwrap();
                            let fmt = match &expr_ty {
                                Ty::Float => self.global_string_ptr("%g", "fmt_g"),
                                Ty::Bool | Ty::String => self.global_string_ptr("%s", "fmt_s"),
                                _ => self.global_string_ptr("%lld", "fmt_lld"),
                            };
                            let val_arg: inkwell::values::BasicMetadataValueEnum = match &expr_ty {
                                Ty::Float => val.into(),
                                Ty::Bool => {
                                    let bv = val.into_int_value();
                                    let true_str = self.global_string_ptr("true", "s_true2");
                                    let false_str = self.global_string_ptr("false", "s_false2");
                                    self.builder
                                        .build_select(bv, true_str, false_str, "boolstr")
                                        .unwrap()
                                        .into()
                                }
                                Ty::String => val.into(),
                                _ => {
                                    let iv = val.into_int_value();
                                    self.builder
                                        .build_int_s_extend_or_bit_cast(iv, self.i64_type(), "ext")
                                        .unwrap()
                                        .into()
                                }
                            };
                            self.builder
                                .build_call(
                                    snprintf_fn,
                                    &[
                                        tmp.into(),
                                        self.i64_type().const_int(64, false).into(),
                                        fmt.into(),
                                        val_arg,
                                    ],
                                    "",
                                )
                                .unwrap();
                            self.builder
                                .build_call(strcat_fn, &[alloca.into(), tmp.into()], "")
                                .unwrap();
                        }
                    }
                }
                // 스택 버퍼를 직접 반환 (GC 없이 함수 수명 동안 유효)
                alloca.into()
            }
        }
    }
}

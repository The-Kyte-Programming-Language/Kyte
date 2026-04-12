use crate::ast::*;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::IntPredicate;
use inkwell::FloatPredicate;
use inkwell::AddressSpace;
use inkwell::basic_block::BasicBlock;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode,
    Target, TargetMachine,
};
use inkwell::OptimizationLevel;
use std::collections::HashMap;
use std::path::Path;

pub struct Codegen<'ctx> {
    context:   &'ctx Context,
    module:    Module<'ctx>,
    builder:   Builder<'ctx>,
    variables: HashMap<String, PointerValue<'ctx>>,
    var_types: HashMap<String, Ty>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    fn_return_tys: HashMap<String, Option<Ty>>,

    /// 배열 길이 추적
    array_lengths: HashMap<String, u64>,

    /// break 시 점프할 블록 (loop/for 용)
    break_bb: Option<BasicBlock<'ctx>>,

    /// 현재 함수
    current_fn: Option<FunctionValue<'ctx>>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let module  = context.create_module("kyte");
        let builder = context.create_builder();

        Codegen {
            context,
            module,
            builder,
            variables: HashMap::new(),
            var_types: HashMap::new(),
            functions: HashMap::new(),
            fn_return_tys: HashMap::new(),
            array_lengths: HashMap::new(),
            break_bb: None,
            current_fn: None,
        }
    }

    // ── 유틸리티 ──

    fn i64_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.i64_type()
    }

    fn f64_type(&self) -> inkwell::types::FloatType<'ctx> {
        self.context.f64_type()
    }

    fn bool_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context.bool_type()
    }

    fn ptr_type(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.ptr_type(AddressSpace::default())
    }

    fn ty_to_llvm(&self, ty: &Ty) -> BasicMetadataTypeEnum<'ctx> {
        match ty {
            Ty::Int | Ty::I64 => self.i64_type().into(),
            Ty::I8            => self.context.i8_type().into(),
            Ty::I16           => self.context.i16_type().into(),
            Ty::I32           => self.context.i32_type().into(),
            Ty::U8            => self.context.i8_type().into(),
            Ty::U16           => self.context.i16_type().into(),
            Ty::U32           => self.context.i32_type().into(),
            Ty::U64           => self.i64_type().into(),
            Ty::Float         => self.f64_type().into(),
            Ty::Bool          => self.bool_type().into(),
            Ty::String        => self.ptr_type().into(),
            Ty::Array(_)      => self.ptr_type().into(),
        }
    }

    /// Ty → LLVM IntType (Int/I8..U64 전용)
    fn ty_to_int_type(&self, ty: &Ty) -> inkwell::types::IntType<'ctx> {
        match ty {
            Ty::I8  | Ty::U8  => self.context.i8_type(),
            Ty::I16 | Ty::U16 => self.context.i16_type(),
            Ty::I32 | Ty::U32 => self.context.i32_type(),
            Ty::Int | Ty::I64 | Ty::U64 => self.i64_type(),
            Ty::Bool => self.bool_type(),
            _ => self.i64_type(),
        }
    }

    /// Ty → BasicTypeEnum (elem_llvm_type 대체)
    fn ty_to_basic(&self, ty: &Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        match ty {
            Ty::I8  | Ty::U8  => self.context.i8_type().into(),
            Ty::I16 | Ty::U16 => self.context.i16_type().into(),
            Ty::I32 | Ty::U32 => self.context.i32_type().into(),
            Ty::Int | Ty::I64 | Ty::U64 => self.i64_type().into(),
            Ty::Float          => self.f64_type().into(),
            Ty::Bool           => self.bool_type().into(),
            Ty::String         => self.ptr_type().into(),
            Ty::Array(_)       => self.ptr_type().into(),
        }
    }

    fn is_signed(ty: &Ty) -> bool {
        matches!(ty, Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
    }

    fn is_integer_ty(ty: &Ty) -> bool {
        matches!(ty, Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64
                    | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64)
    }

    fn declare_printf(&mut self) {
        if self.module.get_function("printf").is_some() { return; }
        let i32_type = self.context.i32_type();
        let fn_type = i32_type.fn_type(&[self.ptr_type().into()], true);
        self.module.add_function("printf", fn_type, Some(inkwell::module::Linkage::External));
    }

    fn declare_exit_fn(&mut self) {
        if self.module.get_function("exit").is_some() { return; }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[self.context.i32_type().into()], false);
        self.module.add_function("exit", fn_type, Some(inkwell::module::Linkage::External));
    }

    fn build_alloca(&self, name: &str, ty: &Ty) -> PointerValue<'ctx> {
        let entry = self.current_fn.unwrap().get_first_basic_block().unwrap();
        let temp_builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(inst) => temp_builder.position_before(&inst),
            None       => temp_builder.position_at_end(entry),
        }
        let llvm_ty = self.ty_to_basic(ty);
        temp_builder.build_alloca(llvm_ty, name).unwrap()
    }

    fn store_var(&self, name: &str, val: BasicValueEnum<'ctx>) {
        let ptr = self.variables[name];
        self.builder.build_store(ptr, val).unwrap();
    }

    fn load_var(&self, name: &str, ty: &Ty) -> BasicValueEnum<'ctx> {
        let ptr = self.variables[name];
        let llvm_ty = self.ty_to_basic(ty);
        self.builder.build_load(llvm_ty, ptr, name).unwrap()
    }

    // ── 프로그램 전체 코드 생성 ──

    pub fn compile(&mut self, program: &Program) {
        self.declare_printf();
        self.declare_exit_fn();

        // 1단계: 함수 프로토타입 선언
        for (item, _) in &program.items {
            if let TopLevel::Function { name, params, return_ty, .. } = item {
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
        }

        // 2단계: 함수 본문 생성
        for (item, _) in &program.items {
            if let TopLevel::Function { name, params, return_ty, body } = item {
                let func = self.functions[name];
                self.current_fn = Some(func);
                let entry = self.context.append_basic_block(func, "entry");
                self.builder.position_at_end(entry);

                let saved_vars = self.variables.clone();
                let saved_types = self.var_types.clone();
                for (i, p) in params.iter().enumerate() {
                    let alloca = self.build_alloca(&p.name, &p.ty);
                    self.builder.build_store(alloca, func.get_nth_param(i as u32).unwrap()).unwrap();
                    self.variables.insert(p.name.clone(), alloca);
                    self.var_types.insert(p.name.clone(), p.ty.clone());
                }

                self.compile_stmts(body, params);

                // 암시적 반환
                if self.no_terminator() {
                    match return_ty {
                        None => { self.builder.build_return(None).unwrap(); }
                        Some(Ty::Float) => { self.builder.build_return(Some(&self.f64_type().const_float(0.0))).unwrap(); }
                        Some(Ty::String) | Some(Ty::Array(_)) => {
                            self.builder.build_return(Some(&self.ptr_type().const_null())).unwrap();
                        }
                        Some(ty) => {
                            let int_ty = self.ty_to_int_type(ty);
                            self.builder.build_return(Some(&int_ty.const_int(0, false))).unwrap();
                        }
                    }
                }

                self.variables = saved_vars;
                self.var_types = saved_types;
            }
        }

        // 3단계: main 앵커 → C main 함수
        for (item, _) in &program.items {
            if let TopLevel::Anchor { kind: AnchorKind::Main, body, children, .. } = item {
                let i32_type = self.context.i32_type();
                let main_fn_type = i32_type.fn_type(&[], false);
                let main_fn = self.module.add_function("main", main_fn_type, None);
                self.current_fn = Some(main_fn);
                let entry = self.context.append_basic_block(main_fn, "entry");
                self.builder.position_at_end(entry);

                self.compile_stmts(body, &[]);

                // 자식 앵커 본문도 인라인
                for (child, _) in children {
                    if let TopLevel::Anchor { body: child_body, children: grandchildren, .. } = child {
                        self.compile_stmts(child_body, &[]);
                        for (gc, _) in grandchildren {
                            if let TopLevel::Anchor { body: gc_body, .. } = gc {
                                self.compile_stmts(gc_body, &[]);
                            }
                        }
                    }
                }

                if self.no_terminator() {
                    self.builder.build_return(Some(&i32_type.const_int(0, false))).unwrap();
                }
            }
        }
    }

    fn no_terminator(&self) -> bool {
        self.builder.get_insert_block()
            .map(|bb| bb.get_terminator().is_none())
            .unwrap_or(true)
    }

    /// IntLit은 항상 i64로 생성되므로, 대상 타입에 맞게 truncate/extend
    fn coerce_to_ty(&self, val: BasicValueEnum<'ctx>, target: &Ty) -> BasicValueEnum<'ctx> {
        if !Self::is_integer_ty(target) || matches!(target, Ty::Int | Ty::I64 | Ty::U64) {
            return val;
        }
        if let BasicValueEnum::IntValue(iv) = val {
            let target_ty = self.ty_to_int_type(target);
            let src_width = iv.get_type().get_bit_width();
            let dst_width = target_ty.get_bit_width();
            if src_width == dst_width { return val; }
            if src_width > dst_width {
                return self.builder.build_int_truncate(iv, target_ty, "trunc").unwrap().into();
            } else if Self::is_signed(target) {
                return self.builder.build_int_s_extend(iv, target_ty, "sext").unwrap().into();
            } else {
                return self.builder.build_int_z_extend(iv, target_ty, "zext").unwrap().into();
            }
        }
        val
    }

    // ── 구문 목록 ──

    fn compile_stmts(&mut self, stmts: &[(Stmt, Span)], params: &[Param]) {
        for (stmt, _) in stmts {
            self.compile_stmt(stmt, params);
            // break/return 후 더 이상 코드 생성하지 않음
            if !self.no_terminator() { break; }
        }
    }

    // ── 개별 구문 ──

    fn compile_stmt(&mut self, stmt: &Stmt, params: &[Param]) {
        match stmt {
            Stmt::VarDecl { ty, name, value } | Stmt::VaultDecl { ty, name, value } => {
                let alloca = self.build_alloca(name, ty);
                let val = self.compile_expr(value, params);
                let val = self.coerce_to_ty(val, ty);
                self.builder.build_store(alloca, val).unwrap();
                self.variables.insert(name.clone(), alloca);
                self.var_types.insert(name.clone(), ty.clone());
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
                    let elem_llvm_ty = self.elem_llvm_type(inner);
                    let gep = unsafe {
                        self.builder.build_gep(elem_llvm_ty, data_ptr, &[idx], "idx_ptr").unwrap()
                    };
                    let val = self.compile_expr(value, params);
                    self.builder.build_store(gep, val).unwrap();
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
                let val = self.compile_expr(e, params);
                self.builder.build_return(Some(&val)).unwrap();
            }
            Stmt::Return(None) => {
                self.builder.build_return(None).unwrap();
            }

            Stmt::If { cond, then_body, else_body } => {
                let func = self.current_fn.unwrap();
                let cond_val = self.compile_expr(cond, params).into_int_value();
                let then_bb  = self.context.append_basic_block(func, "then");
                let else_bb  = self.context.append_basic_block(func, "else");
                let merge_bb = self.context.append_basic_block(func, "merge");

                self.builder.build_conditional_branch(cond_val, then_bb, else_bb).unwrap();

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
                let func     = self.current_fn.unwrap();
                let loop_bb  = self.context.append_basic_block(func, "loop");
                let after_bb = self.context.append_basic_block(func, "after_loop");

                let saved_break = self.break_bb;
                self.break_bb = Some(after_bb);

                self.builder.build_unconditional_branch(loop_bb).unwrap();
                self.builder.position_at_end(loop_bb);
                self.compile_stmts(body, params);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(loop_bb).unwrap();
                }

                self.break_bb = saved_break;
                self.builder.position_at_end(after_bb);
            }

            Stmt::For { var, from, to, body } => {
                let func         = self.current_fn.unwrap();
                let _preheader_bb = self.builder.get_insert_block().unwrap();
                let loop_bb      = self.context.append_basic_block(func, "for_body");
                let after_bb     = self.context.append_basic_block(func, "for_after");

                // 초기값
                let start_val = self.compile_expr(from, params).into_int_value();
                let end_val   = self.compile_expr(to, params).into_int_value();

                let saved_break = self.break_bb;
                self.break_bb = Some(after_bb);

                // alloca for loop var
                let alloca = self.build_alloca(var, &Ty::Int);
                self.builder.build_store(alloca, start_val).unwrap();
                self.variables.insert(var.clone(), alloca);
                self.var_types.insert(var.clone(), Ty::Int);

                // 진입 조건
                let cond = self.builder.build_int_compare(
                    IntPredicate::SLT, start_val, end_val, "for_cond"
                ).unwrap();
                self.builder.build_conditional_branch(cond, loop_bb, after_bb).unwrap();

                // body
                self.builder.position_at_end(loop_bb);
                self.compile_stmts(body, params);

                if self.no_terminator() {
                    // increment
                    let cur = self.builder.build_load(self.i64_type(), alloca, var).unwrap().into_int_value();
                    let next = self.builder.build_int_add(
                        cur, self.i64_type().const_int(1, false), "next"
                    ).unwrap();
                    self.builder.build_store(alloca, next).unwrap();

                    let loop_cond = self.builder.build_int_compare(
                        IntPredicate::SLT, next, end_val, "for_cond"
                    ).unwrap();
                    self.builder.build_conditional_branch(loop_cond, loop_bb, after_bb).unwrap();
                }

                self.break_bb = saved_break;
                self.builder.position_at_end(after_bb);
            }

            Stmt::Break => {
                if let Some(bb) = self.break_bb {
                    self.builder.build_unconditional_branch(bb).unwrap();
                }
            }

            Stmt::Exit => {
                let exit_fn = self.module.get_function("exit").unwrap();
                self.builder.build_call(
                    exit_fn,
                    &[self.context.i32_type().const_int(0, false).into()],
                    "",
                ).unwrap();
                self.builder.build_unreachable().unwrap();
            }

            Stmt::Kill(_) => {
                let exit_fn = self.module.get_function("exit").unwrap();
                self.builder.build_call(
                    exit_fn,
                    &[self.context.i32_type().const_int(1, false).into()],
                    "",
                ).unwrap();
                self.builder.build_unreachable().unwrap();
            }

            Stmt::Free(_) => {
                // Vault free — 현재는 no-op (leak 아닌 stack alloc이므로)
            }

            Stmt::Yield(e) => {
                // yield → print
                let val = self.compile_expr(e, params);
                let ty = self.guess_expr_ty(e, params);
                self.emit_print(val, Some(&ty));
            }

            Stmt::InlineAnchor { body, .. } => {
                self.compile_stmts(body, params);
            }

            Stmt::ExprStmt(e) => {
                self.compile_expr(e, params);
            }
        }
    }

    // ── printf 헬퍼 ──

    fn emit_print(&self, val: BasicValueEnum<'ctx>, ty: Option<&Ty>) {
        let printf = self.module.get_function("printf").unwrap();
        match val {
            BasicValueEnum::IntValue(iv) => {
                let width = iv.get_type().get_bit_width();
                if width == 1 {
                    // bool(i1)
                    let fmt = self.builder.build_global_string_ptr("%s\n", "fmt_bool").unwrap();
                    let true_str  = self.builder.build_global_string_ptr("true", "s_true").unwrap();
                    let false_str = self.builder.build_global_string_ptr("false", "s_false").unwrap();
                    let selected = self.builder.build_select(
                        iv,
                        true_str.as_pointer_value(),
                        false_str.as_pointer_value(),
                        "sel",
                    ).unwrap();
                    self.builder.build_call(
                        printf,
                        &[fmt.as_pointer_value().into(), selected.into()],
                        "",
                    ).unwrap();
                } else {
                    // i8~i64, u8~u64 → extend to i64 for printf
                    let print_val = if width < 64 {
                        let is_unsigned = matches!(ty,
                            Some(Ty::U8) | Some(Ty::U16) | Some(Ty::U32) | Some(Ty::U64)
                        );
                        if is_unsigned {
                            self.builder.build_int_z_extend(iv, self.i64_type(), "ext_print").unwrap()
                        } else {
                            self.builder.build_int_s_extend(iv, self.i64_type(), "ext_print").unwrap()
                        }
                    } else {
                        iv
                    };
                    let fmt = self.builder.build_global_string_ptr("%lld\n", "fmt_int").unwrap();
                    self.builder.build_call(
                        printf,
                        &[fmt.as_pointer_value().into(), print_val.into()],
                        "",
                    ).unwrap();
                }
            }
            BasicValueEnum::FloatValue(fv) => {
                let fmt = self.builder.build_global_string_ptr("%f\n", "fmt_float").unwrap();
                self.builder.build_call(
                    printf,
                    &[fmt.as_pointer_value().into(), fv.into()],
                    "",
                ).unwrap();
            }
            BasicValueEnum::PointerValue(pv) => {
                let fmt = self.builder.build_global_string_ptr("%s\n", "fmt_str").unwrap();
                self.builder.build_call(
                    printf,
                    &[fmt.as_pointer_value().into(), pv.into()],
                    "",
                ).unwrap();
            }
            _ => {}
        }
    }

    // ── 표현식 ──

    fn compile_expr(&mut self, expr: &Expr, params: &[Param]) -> BasicValueEnum<'ctx> {
        match expr {
            Expr::IntLit(n) => {
                self.i64_type().const_int(*n as u64, true).into()
            }
            Expr::FloatLit(f) => {
                self.f64_type().const_float(*f).into()
            }
            Expr::StringLit(s) => {
                let global = self.builder.build_global_string_ptr(s, "str").unwrap();
                global.as_pointer_value().into()
            }
            Expr::Bool(b) => {
                self.bool_type().const_int(if *b { 1 } else { 0 }, false).into()
            }
            Expr::Ident(name) => {
                let ty = self.guess_var_ty(name, params);
                self.load_var(name, &ty)
            }
            Expr::UnaryOp { op, expr } => {
                let val = self.compile_expr(expr, params);
                match op {
                    UnaryOpKind::Neg => {
                        match val {
                            BasicValueEnum::IntValue(iv) =>
                                self.builder.build_int_neg(iv, "neg").unwrap().into(),
                            BasicValueEnum::FloatValue(fv) =>
                                self.builder.build_float_neg(fv, "fneg").unwrap().into(),
                            _ => val,
                        }
                    }
                    UnaryOpKind::Not => {
                        let iv = val.into_int_value();
                        self.builder.build_not(iv, "not").unwrap().into()
                    }
                }
            }
            Expr::BinOp { left, op, right } => {
                let mut l = self.compile_expr(left, params);
                let mut r = self.compile_expr(right, params);

                // string + string → concat (간단한 구현은 skip — 문자열 포인터로 유지)
                // 여기선 int/float/bool 만 처리
                let ty = self.guess_expr_ty(left, params);
                if Self::is_integer_ty(&ty) {
                    l = self.coerce_to_ty(l, &ty);
                    r = self.coerce_to_ty(r, &ty);
                }
                self.compile_binop(op, l, r, &ty)
            }
            Expr::Call { name, args } => {
                // len() 빌트인
                if name == "len" {
                    if let Some(arg_name) = match &args[0] {
                        Expr::Ident(n) => Some(n.clone()),
                        _ => None,
                    } {
                        let len = self.array_lengths.get(&arg_name).copied().unwrap_or(0);
                        return self.i64_type().const_int(len, false).into();
                    }
                    return self.i64_type().const_int(0, false).into();
                }

                let func = self.functions[name];
                let compiled_args: Vec<BasicMetadataValueEnum> =
                    args.iter().map(|a| self.compile_expr(a, params).into()).collect();
                let call_site = self.builder.build_call(func, &compiled_args, &format!("{}_ret", name)).unwrap();
                call_site.try_as_basic_value()
                    .basic()
                    .unwrap_or_else(|| self.i64_type().const_int(0, false).into())
            }

            Expr::ArrayLit(elems) => {
                let elem_ty = self.guess_expr_ty(&elems[0], params);
                let elem_llvm_ty = self.elem_llvm_type(&elem_ty);
                let count = elems.len() as u64;
                let size = self.i64_type().const_int(count, false);
                let data_ptr = self.builder.build_array_alloca(elem_llvm_ty, size, "arr_data").unwrap();
                for (i, elem) in elems.iter().enumerate() {
                    let val = self.compile_expr(elem, params);
                    let idx = self.i64_type().const_int(i as u64, false);
                    let gep = unsafe {
                        self.builder.build_gep(elem_llvm_ty, data_ptr, &[idx], "arr_elem").unwrap()
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
                let data_ptr = self.compile_expr(array, params).into_pointer_value();
                let idx = self.compile_expr(index, params).into_int_value();
                let elem_llvm_ty = self.elem_llvm_type(&inner);
                let gep = unsafe {
                    self.builder.build_gep(elem_llvm_ty, data_ptr, &[idx], "idx_ptr").unwrap()
                };
                self.builder.build_load(elem_llvm_ty, gep, "idx_val").unwrap()
            }
        }
    }

    fn compile_binop(&self, op: &BinOpKind, l: BasicValueEnum<'ctx>, r: BasicValueEnum<'ctx>, ty: &Ty) -> BasicValueEnum<'ctx> {
        if Self::is_integer_ty(ty) || matches!(ty, Ty::Array(_)) {
            let li = l.into_int_value();
            let ri = r.into_int_value();
            let signed = Self::is_signed(ty);
            match op {
                BinOpKind::Add => self.builder.build_int_add(li, ri, "add").unwrap().into(),
                BinOpKind::Sub => self.builder.build_int_sub(li, ri, "sub").unwrap().into(),
                BinOpKind::Mul => self.builder.build_int_mul(li, ri, "mul").unwrap().into(),
                BinOpKind::Div => {
                    if signed {
                        self.builder.build_int_signed_div(li, ri, "sdiv").unwrap().into()
                    } else {
                        self.builder.build_int_unsigned_div(li, ri, "udiv").unwrap().into()
                    }
                }
                BinOpKind::Mod => {
                    if signed {
                        self.builder.build_int_signed_rem(li, ri, "srem").unwrap().into()
                    } else {
                        self.builder.build_int_unsigned_rem(li, ri, "urem").unwrap().into()
                    }
                }
                BinOpKind::Lt => {
                    let pred = if signed { IntPredicate::SLT } else { IntPredicate::ULT };
                    self.builder.build_int_compare(pred, li, ri, "lt").unwrap().into()
                }
                BinOpKind::Gt => {
                    let pred = if signed { IntPredicate::SGT } else { IntPredicate::UGT };
                    self.builder.build_int_compare(pred, li, ri, "gt").unwrap().into()
                }
                BinOpKind::Le => {
                    let pred = if signed { IntPredicate::SLE } else { IntPredicate::ULE };
                    self.builder.build_int_compare(pred, li, ri, "le").unwrap().into()
                }
                BinOpKind::Ge => {
                    let pred = if signed { IntPredicate::SGE } else { IntPredicate::UGE };
                    self.builder.build_int_compare(pred, li, ri, "ge").unwrap().into()
                }
                BinOpKind::Eq  => self.builder.build_int_compare(IntPredicate::EQ, li, ri, "eq").unwrap().into(),
                BinOpKind::Neq => self.builder.build_int_compare(IntPredicate::NE, li, ri, "ne").unwrap().into(),
                BinOpKind::And => self.builder.build_and(li, ri, "and").unwrap().into(),
                BinOpKind::Or  => self.builder.build_or(li, ri, "or").unwrap().into(),
            }
        } else {
            match ty {
                Ty::Float => {
                    let lf = l.into_float_value();
                    let rf = r.into_float_value();
                    match op {
                        BinOpKind::Add => self.builder.build_float_add(lf, rf, "fadd").unwrap().into(),
                        BinOpKind::Sub => self.builder.build_float_sub(lf, rf, "fsub").unwrap().into(),
                        BinOpKind::Mul => self.builder.build_float_mul(lf, rf, "fmul").unwrap().into(),
                        BinOpKind::Div => self.builder.build_float_div(lf, rf, "fdiv").unwrap().into(),
                        BinOpKind::Mod => self.builder.build_float_rem(lf, rf, "fmod").unwrap().into(),
                        BinOpKind::Lt  => self.builder.build_float_compare(FloatPredicate::OLT, lf, rf, "flt").unwrap().into(),
                        BinOpKind::Gt  => self.builder.build_float_compare(FloatPredicate::OGT, lf, rf, "fgt").unwrap().into(),
                        BinOpKind::Le  => self.builder.build_float_compare(FloatPredicate::OLE, lf, rf, "fle").unwrap().into(),
                        BinOpKind::Ge  => self.builder.build_float_compare(FloatPredicate::OGE, lf, rf, "fge").unwrap().into(),
                        BinOpKind::Eq  => self.builder.build_float_compare(FloatPredicate::OEQ, lf, rf, "feq").unwrap().into(),
                        BinOpKind::Neq => self.builder.build_float_compare(FloatPredicate::ONE, lf, rf, "fne").unwrap().into(),
                        _ => l,
                    }
                }
                Ty::Bool => {
                let li = l.into_int_value();
                let ri = r.into_int_value();
                match op {
                    BinOpKind::And => self.builder.build_and(li, ri, "band").unwrap().into(),
                    BinOpKind::Or  => self.builder.build_or(li, ri, "bor").unwrap().into(),
                    BinOpKind::Eq  => self.builder.build_int_compare(IntPredicate::EQ, li, ri, "beq").unwrap().into(),
                    BinOpKind::Neq => self.builder.build_int_compare(IntPredicate::NE, li, ri, "bne").unwrap().into(),
                    _ => l,
                }
            }
            Ty::String => {
                // string ops — 지금은 포인터 그대로 반환
                l
            }
            _ => l,
            }
        }
    }

    // ── 타입 추론 (codegen 시점, 간단 버전) ──

    fn elem_llvm_type(&self, ty: &Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        self.ty_to_basic(ty)
    }

    fn guess_var_ty(&self, name: &str, params: &[Param]) -> Ty {
        if let Some(ty) = self.var_types.get(name) {
            return ty.clone();
        }
        for p in params {
            if p.name == name { return p.ty.clone(); }
        }
        Ty::Int
    }

    fn guess_expr_ty(&self, expr: &Expr, params: &[Param]) -> Ty {
        match expr {
            Expr::IntLit(_)    => Ty::Int,
            Expr::FloatLit(_)  => Ty::Float,
            Expr::StringLit(_) => Ty::String,
            Expr::Bool(_)      => Ty::Bool,
            Expr::Ident(name)  => self.guess_var_ty(name, params),
            Expr::UnaryOp { op, expr } => {
                match op {
                    UnaryOpKind::Not => Ty::Bool,
                    UnaryOpKind::Neg => self.guess_expr_ty(expr, params),
                }
            }
            Expr::BinOp { left, op, .. } => {
                match op {
                    BinOpKind::Lt | BinOpKind::Gt | BinOpKind::Le
                    | BinOpKind::Ge | BinOpKind::Eq | BinOpKind::Neq
                    | BinOpKind::And | BinOpKind::Or => Ty::Bool,
                    _ => self.guess_expr_ty(left, params),
                }
            }
            Expr::Call { name, .. } => {
                self.fn_return_tys.get(name)
                    .and_then(|opt| opt.clone())
                    .unwrap_or(Ty::Int)
            }
            Expr::ArrayLit(elems) => {
                if elems.is_empty() { Ty::Array(Box::new(Ty::Int)) }
                else { Ty::Array(Box::new(self.guess_expr_ty(&elems[0], params))) }
            }
            Expr::Index { array, .. } => {
                match self.guess_expr_ty(array, params) {
                    Ty::Array(inner) => *inner,
                    _ => Ty::Int,
                }
            }
        }
    }

    // ── 출력 ──

    pub fn get_ir_string(&self) -> String {
        self.module.print_to_string().to_string()
    }

    pub fn print_ir(&self) {
        println!("{}", self.module.print_to_string().to_string());
    }

    pub fn write_object_file(&self, path: &str) {
        Target::initialize_native(&InitializationConfig::default())
            .expect("Failed to initialize native target");

        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).expect("Failed to get target");
        let machine = target.create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        ).expect("Failed to create target machine");

        machine.write_to_file(&self.module, FileType::Object, Path::new(path))
            .expect("Failed to write object file");
    }

    pub fn write_ir_file(&self, path: &str) {
        self.module.print_to_file(Path::new(path)).expect("Failed to write IR file");
    }
}

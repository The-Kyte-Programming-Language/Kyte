use crate::ast::*;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Module;
use inkwell::types::StructType;
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use std::collections::{HashMap, HashSet};

#[path = "codegen/ops.rs"]
mod ops;
#[path = "codegen/runtime.rs"]
mod runtime;

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    variables: HashMap<String, PointerValue<'ctx>>,
    var_types: HashMap<String, Ty>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    fn_return_tys: HashMap<String, Option<Ty>>,
    struct_defs: HashMap<String, Vec<StructField>>,
    struct_types: HashMap<String, StructType<'ctx>>,
    string_globals: HashMap<String, PointerValue<'ctx>>,
    string_global_counter: u64,
    struct_size_cache: HashMap<String, u64>,

    /// enum 정의 (variants)
    enum_defs: HashMap<String, Vec<crate::ast::EnumVariant>>,
    /// enum LLVM 타입 { i32 tag, [max_payload_size x i8] }
    enum_types: HashMap<String, StructType<'ctx>>,
    /// enum variant 태그 매핑: enum_name -> variant_name -> tag_index
    enum_variant_tags: HashMap<String, HashMap<String, u32>>,
    /// enum payload 크기
    enum_payload_sizes: HashMap<String, u64>,

    /// 배열 길이 추적
    array_lengths: HashMap<String, u64>,

    /// Vault 변수 (힙 할당) 추적
    vault_vars: std::collections::HashSet<String>,
    freed_vault_vars: std::collections::HashSet<String>,
    vault_scope_stack: Vec<Vec<String>>,
    break_cleanup_depth: Option<usize>,

    /// break 시 점프할 블록 (loop/for 용)
    break_bb: Option<BasicBlock<'ctx>>,

    /// Kill 시 점프할 복구 블록 스택 (anchor recovery)
    recovery_stack: Vec<BasicBlock<'ctx>>,

    /// recovery_stack과 1:1 대응되는 Vault cleanup depth
    kill_cleanup_depth_stack: Vec<usize>,

    /// yield 값을 저장할 alloca 스택 (anchor yield)
    yield_slot: Vec<PointerValue<'ctx>>,

    /// yield 후 점프할 블록 스택
    yield_merge_bb: Vec<BasicBlock<'ctx>>,

    /// 앵커별 Kill 발생 횟수 슬롯
    kill_count_slot: Vec<PointerValue<'ctx>>,

    /// Vault 런타임 카운터 (recovery assert용)
    vault_live_count: Option<PointerValue<'ctx>>,

    /// 현재 함수
    current_fn: Option<FunctionValue<'ctx>>,

    /// 디버그 모드 — overflow trap / 상세 런타임 검사 활성화
    pub debug_mode: bool,

    /// 최적화 레벨 (A03)
    pub opt_level: OptimizationLevel,

    /// 임시 문자열 힙 버퍼 (concat / to_string_ptr) 추적 — scope cleanup 시 해제 (M05)
    string_temp_stack: Vec<Vec<PointerValue<'ctx>>>,

    /// 클로저 익명 함수 카운터
    closure_counter: u64,

    /// 등록된 모듈 이름 목록 (mod call 해석용)
    module_names: std::collections::HashSet<String>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("kyte");
        let builder = context.create_builder();

        Codegen {
            context,
            module,
            builder,
            variables: HashMap::new(),
            var_types: HashMap::new(),
            functions: HashMap::new(),
            fn_return_tys: HashMap::new(),
            struct_defs: HashMap::new(),
            struct_types: HashMap::new(),
            string_globals: HashMap::new(),
            string_global_counter: 0,
            struct_size_cache: HashMap::new(),
            enum_defs: HashMap::new(),
            enum_types: HashMap::new(),
            enum_variant_tags: HashMap::new(),
            enum_payload_sizes: HashMap::new(),
            array_lengths: HashMap::new(),
            vault_vars: std::collections::HashSet::new(),
            freed_vault_vars: std::collections::HashSet::new(),
            vault_scope_stack: Vec::new(),
            break_cleanup_depth: None,
            break_bb: None,
            recovery_stack: Vec::new(),
            kill_cleanup_depth_stack: Vec::new(),
            yield_slot: Vec::new(),
            yield_merge_bb: Vec::new(),
            kill_count_slot: Vec::new(),
            vault_live_count: None,
            current_fn: None,
            debug_mode: true,
            opt_level: OptimizationLevel::None,
            string_temp_stack: Vec::new(),
            closure_counter: 0,
            module_names: std::collections::HashSet::new(),
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
            Ty::I8 => self.context.i8_type().into(),
            Ty::I16 => self.context.i16_type().into(),
            Ty::I32 => self.context.i32_type().into(),
            Ty::U8 => self.context.i8_type().into(),
            Ty::U16 => self.context.i16_type().into(),
            Ty::U32 => self.context.i32_type().into(),
            Ty::U64 => self.i64_type().into(),
            Ty::Float => self.f64_type().into(),
            Ty::Bool => self.bool_type().into(),
            Ty::String => self.ptr_type().into(),
            Ty::Array(_) => self.ptr_type().into(),
            Ty::Struct(name) => self.struct_types[name].into(),
            Ty::Enum(name) => self
                .enum_types
                .get(name)
                .map(|t| (*t).into())
                .unwrap_or(self.i64_type().into()),
            Ty::Auto | Ty::TypeParam(_) | Ty::Fn(_, _) => self.i64_type().into(), // fallback
        }
    }

    /// Ty → LLVM IntType (Int/I8..U64 전용)
    fn ty_to_int_type(&self, ty: &Ty) -> inkwell::types::IntType<'ctx> {
        match ty {
            Ty::I8 | Ty::U8 => self.context.i8_type(),
            Ty::I16 | Ty::U16 => self.context.i16_type(),
            Ty::I32 | Ty::U32 => self.context.i32_type(),
            Ty::Int | Ty::I64 | Ty::U64 => self.i64_type(),
            Ty::Bool => self.bool_type(),
            Ty::Enum(_) | Ty::Auto | Ty::TypeParam(_) => self.i64_type(),
            _ => self.i64_type(),
        }
    }

    /// Ty → BasicTypeEnum (elem_llvm_type 대체)
    fn ty_to_basic(&self, ty: &Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        match ty {
            Ty::I8 | Ty::U8 => self.context.i8_type().into(),
            Ty::I16 | Ty::U16 => self.context.i16_type().into(),
            Ty::I32 | Ty::U32 => self.context.i32_type().into(),
            Ty::Int | Ty::I64 | Ty::U64 => self.i64_type().into(),
            Ty::Float => self.f64_type().into(),
            Ty::Bool => self.bool_type().into(),
            Ty::String => self.ptr_type().into(),
            Ty::Array(_) => self.ptr_type().into(),
            Ty::Struct(name) => self.struct_types[name].into(),
            Ty::Enum(name) => self
                .enum_types
                .get(name)
                .map(|t| (*t).into())
                .unwrap_or(self.i64_type().into()),
            Ty::Auto | Ty::TypeParam(_) | Ty::Fn(_, _) => self.i64_type().into(), // fallback
        }
    }

    fn is_signed(ty: &Ty) -> bool {
        matches!(ty, Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
    }

    fn is_integer_ty(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
        )
    }

    fn build_alloca(&self, name: &str, ty: &Ty) -> PointerValue<'ctx> {
        let entry = self.current_fn.unwrap().get_first_basic_block().unwrap();
        let temp_builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(inst) => temp_builder.position_before(&inst),
            None => temp_builder.position_at_end(entry),
        }
        let llvm_ty = self.ty_to_basic(ty);
        temp_builder.build_alloca(llvm_ty, name).unwrap()
    }

    fn global_string_ptr(&mut self, value: &str, prefix: &str) -> PointerValue<'ctx> {
        if let Some(ptr) = self.string_globals.get(value).copied() {
            return ptr;
        }

        let name = format!("{}_{}", prefix, self.string_global_counter);
        self.string_global_counter += 1;
        let ptr = self
            .builder
            .build_global_string_ptr(value, &name)
            .unwrap()
            .as_pointer_value();
        self.string_globals.insert(value.to_string(), ptr);
        ptr
    }

    fn type_size_bytes(&mut self, ty: &Ty) -> u64 {
        let mut visiting = HashSet::new();
        self.type_size_bytes_with_visiting(ty, &mut visiting)
    }

    fn type_size_bytes_with_visiting(&mut self, ty: &Ty, visiting: &mut HashSet<String>) -> u64 {
        match ty {
            Ty::I8 | Ty::U8 | Ty::Bool => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 => 4,
            Ty::Int | Ty::I64 | Ty::U64 => 8,
            Ty::Float => 8,
            Ty::String | Ty::Array(_) => 8, // pointer size
            Ty::Struct(name) => self.struct_size_bytes(name, visiting),
            Ty::Enum(name) => {
                let payload = self.enum_payload_sizes.get(name).copied().unwrap_or(0);
                4 + payload // tag (i32) + payload
            }
            Ty::Auto | Ty::TypeParam(_) | Ty::Fn(_, _) => 8, // fallback
        }
    }

    fn struct_size_bytes(&mut self, name: &str, visiting: &mut HashSet<String>) -> u64 {
        if let Some(size) = self.struct_size_cache.get(name).copied() {
            return size;
        }
        if !visiting.insert(name.to_string()) {
            return 8;
        }

        let size = if let Some(fields) = self.struct_defs.get(name).cloned() {
            let total = fields
                .iter()
                .map(|field| self.type_size_bytes_with_visiting(&field.ty, visiting))
                .sum::<u64>();
            total.max(1)
        } else {
            8
        };

        visiting.remove(name);
        self.struct_size_cache.insert(name.to_string(), size);
        size
    }

    fn struct_field_info(&self, sname: &str, field: &str) -> Option<(u32, Ty)> {
        let fields = self.struct_defs.get(sname)?;
        for (idx, f) in fields.iter().enumerate() {
            if f.name == field {
                return Some((idx as u32, f.ty.clone()));
            }
        }
        None
    }

    fn store_var(&self, name: &str, val: BasicValueEnum<'ctx>) {
        let ptr = self.variables[name];
        if self.vault_vars.contains(name) {
            // vault: alloca → heap pointer, store value to heap
            let heap_ptr = self
                .builder
                .build_load(self.ptr_type(), ptr, &format!("{}_ptr", name))
                .unwrap()
                .into_pointer_value();
            self.builder.build_store(heap_ptr, val).unwrap();
        } else {
            self.builder.build_store(ptr, val).unwrap();
        }
    }

    fn load_var(&self, name: &str, ty: &Ty) -> BasicValueEnum<'ctx> {
        if let Some(&ptr) = self.variables.get(name) {
            if self.vault_vars.contains(name) {
                // vault: alloca → heap pointer → actual value
                let heap_ptr = self
                    .builder
                    .build_load(self.ptr_type(), ptr, &format!("{}_ptr", name))
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

    fn free_vault_var(&mut self, name: &str) {
        if !self.vault_vars.contains(name) {
            return;
        }
        if self.freed_vault_vars.contains(name) {
            return;
        }
        if let Some(ptr) = self.variables.get(name).copied() {
            let heap_ptr = self
                .builder
                .build_load(self.ptr_type(), ptr, &format!("{}_ptr", name))
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

    fn register_vault_in_current_scope(&mut self, name: &str) {
        if let Some(scope) = self.vault_scope_stack.last_mut() {
            scope.push(name.to_string());
        }
    }

    /// 앵커 진입 시 vault 런타임 카운터를 저장
    fn save_vault_count(&mut self, label: &str) -> Option<PointerValue<'ctx>> {
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

    /// recovery 블록 진입 시 vault 카운터 assert 검증
    fn emit_recovery_vault_assert(
        &mut self,
        merge_bb: BasicBlock<'ctx>,
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
        self.builder.build_unconditional_branch(merge_bb).unwrap();
    }

    fn emit_null_check(&mut self, ptr: PointerValue<'ctx>, label: &str) {
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
    fn emit_overflow_check(
        &mut self,
        li: inkwell::values::IntValue<'ctx>,
        ri: inkwell::values::IntValue<'ctx>,
        op_name: &str,
        result_name: &str,
    ) -> BasicValueEnum<'ctx> {
        use inkwell::intrinsics::Intrinsic;
        let int_ty = li.get_type();
        let intrinsic_name = format!("llvm.{}.with.overflow", op_name);
        if let Some(intrinsic) = Intrinsic::find(&intrinsic_name) {
            let fn_val = intrinsic
                .get_declaration(&self.module, &[int_ty.into()])
                .expect("overflow intrinsic declaration");

            let call = self
                .builder
                .build_call(fn_val, &[li.into(), ri.into()], &format!("{}_ovf", op_name))
                .unwrap();
            let struct_val = call
                .try_as_basic_value()
                .basic()
                .unwrap()
                .into_struct_value();

            let result = self
                .builder
                .build_extract_value(struct_val, 0, result_name)
                .unwrap()
                .into_int_value();
            let overflow = self
                .builder
                .build_extract_value(struct_val, 1, &format!("{}_ovf_flag", op_name))
                .unwrap()
                .into_int_value();

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
        } else {
            // fallback: intrinsic 없을 경우 일반 연산
            match op_name {
                "sadd" => self
                    .builder
                    .build_int_add(li, ri, result_name)
                    .unwrap()
                    .into(),
                "ssub" => self
                    .builder
                    .build_int_sub(li, ri, result_name)
                    .unwrap()
                    .into(),
                _ => self
                    .builder
                    .build_int_mul(li, ri, result_name)
                    .unwrap()
                    .into(),
            }
        }
    }

    fn emit_bounds_check(
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
    fn emit_negative_index_check(&mut self, idx: inkwell::values::IntValue<'ctx>, arr_name: &str) {
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
            .build_call(printf, &[fmt.into(), name_ptr.into(), idx.into()], "")
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

    fn cleanup_current_scope(&mut self) {
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

    fn cleanup_to_depth(&mut self, target_depth: usize) {
        while self.vault_scope_stack.len() > target_depth {
            self.cleanup_current_scope();
        }
    }

    // ── 프로그램 전체 코드 생성 ──

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

    fn no_terminator(&self) -> bool {
        self.builder
            .get_insert_block()
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
            if src_width == dst_width {
                return val;
            }
            if src_width > dst_width {
                return self
                    .builder
                    .build_int_truncate(iv, target_ty, "trunc")
                    .unwrap()
                    .into();
            } else if Self::is_signed(target) {
                return self
                    .builder
                    .build_int_s_extend(iv, target_ty, "sext")
                    .unwrap()
                    .into();
            } else {
                return self
                    .builder
                    .build_int_z_extend(iv, target_ty, "zext")
                    .unwrap()
                    .into();
            }
        }
        val
    }

    // ── 구문 목록 ──

    fn compile_stmts(&mut self, stmts: &[(Stmt, Span)], params: &[Param]) {
        let start_depth = self.vault_scope_stack.len();
        self.vault_scope_stack.push(Vec::new());
        self.string_temp_stack.push(Vec::new());

        for (stmt, _) in stmts {
            self.compile_stmt(stmt, params);

            if let Stmt::VaultDecl { name, .. } = stmt {
                self.register_vault_in_current_scope(name);
            }

            // break/return 후 더 이상 코드 생성하지 않음
            if !self.no_terminator() {
                break;
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

    // ── 개별 구문 ──

    fn compile_stmt(&mut self, stmt: &Stmt, params: &[Param]) {
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
                    .build_call(malloc, &[size_val.into()], &format!("{}_heap", name))
                    .unwrap()
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_pointer_value();

                // NULL 체크 (C03)
                self.emit_null_check(heap_ptr, &format!("vault_{}", name));

                // 값 계산 후 heap에 저장
                let val = self.compile_expr(value, params);
                let val = self.coerce_to_ty(val, ty);
                self.builder.build_store(heap_ptr, val).unwrap();

                // 변수는 힙 포인터를 저장하는 alloca (pointer 타입으
                let entry = self.current_fn.unwrap().get_first_basic_block().unwrap();
                let temp_builder = self.context.create_builder();
                match entry.get_first_instruction() {
                    Some(inst) => temp_builder.position_before(&inst),
                    None => temp_builder.position_at_end(entry),
                }
                let alloca = temp_builder.build_alloca(self.ptr_type(), name).unwrap();
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
                                .build_load(
                                    self.ptr_type(),
                                    self.variables[name],
                                    &format!("{}_ptr", name),
                                )
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
                self.break_bb = Some(after_bb);
                self.break_cleanup_depth = Some(self.vault_scope_stack.len());

                self.builder.build_unconditional_branch(loop_bb).unwrap();
                self.builder.position_at_end(loop_bb);
                self.compile_stmts(body, params);
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(loop_bb).unwrap();
                }

                self.break_bb = saved_break;
                self.break_cleanup_depth = saved_break_depth;
                self.builder.position_at_end(after_bb);
            }

            Stmt::While { cond, body } => {
                let func = self.current_fn.unwrap();
                let cond_bb = self.context.append_basic_block(func, "while_cond");
                let body_bb = self.context.append_basic_block(func, "while_body");
                let after_bb = self.context.append_basic_block(func, "while_after");

                let saved_break = self.break_bb;
                let saved_break_depth = self.break_cleanup_depth;
                self.break_bb = Some(after_bb);
                self.break_cleanup_depth = Some(self.vault_scope_stack.len());

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
                self.builder.position_at_end(after_bb);
            }

            Stmt::For {
                var,
                from,
                to,
                body,
            } => {
                let func = self.current_fn.unwrap();
                let _preheader_bb = self.builder.get_insert_block().unwrap();
                let loop_bb = self.context.append_basic_block(func, "for_body");
                let after_bb = self.context.append_basic_block(func, "for_after");

                // 초기값
                let start_val = self.compile_expr(from, params).into_int_value();
                let end_val = self.compile_expr(to, params).into_int_value();

                let saved_break = self.break_bb;
                let saved_break_depth = self.break_cleanup_depth;
                self.break_bb = Some(after_bb);
                self.break_cleanup_depth = Some(self.vault_scope_stack.len());

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
                    // increment
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
                }

                self.break_bb = saved_break;
                self.break_cleanup_depth = saved_break_depth;
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
                if let Some(e) = msg {
                    let val = self.compile_expr(e, params);
                    let ty = self.guess_expr_ty(e, params);
                    self.emit_print(val, Some(&ty));
                }
                if let Some(&recovery_bb) = self.recovery_stack.last() {
                    let normal_depth = self.kill_cleanup_depth_stack.last().copied().unwrap_or(0);
                    // 같은 앵커에서 Kill 3회 이상이면 상위 앵커 복구로 승격
                    let mut target_bb = recovery_bb;
                    let mut target_depth = normal_depth;
                    if let Some(&counter_ptr) = self.kill_count_slot.last() {
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

                        if self.recovery_stack.len() >= 2 {
                            let escalate_bb = self.recovery_stack[self.recovery_stack.len() - 2];
                            let escalate_depth = self
                                .kill_cleanup_depth_stack
                                .get(self.kill_cleanup_depth_stack.len() - 2)
                                .copied()
                                .unwrap_or(0);
                            let escalate_cond = self
                                .builder
                                .build_int_compare(
                                    IntPredicate::UGE,
                                    next,
                                    self.i64_type().const_int(3, false),
                                    "kill_escalate_cond",
                                )
                                .unwrap();
                            let normal_bb = self.context.append_basic_block(
                                self.current_fn.unwrap(),
                                "kill_normal_recover",
                            );
                            let escalated_bb = self.context.append_basic_block(
                                self.current_fn.unwrap(),
                                "kill_escalated_recover",
                            );
                            self.builder
                                .build_conditional_branch(escalate_cond, escalated_bb, normal_bb)
                                .unwrap();

                            self.builder.position_at_end(normal_bb);
                            self.cleanup_to_depth(normal_depth);
                            self.builder
                                .build_unconditional_branch(recovery_bb)
                                .unwrap();
                            self.builder.position_at_end(escalated_bb);
                            self.cleanup_to_depth(escalate_depth);
                            target_bb = escalate_bb;
                            target_depth = escalate_depth;
                        }
                    }
                    if self.no_terminator() {
                        self.cleanup_to_depth(target_depth);
                    }
                    self.builder.build_unconditional_branch(target_bb).unwrap();
                }
            }

            Stmt::Free(_name) => {
                // 자동 메모리 관리: free()는 무시 (scope exit 시 자동 해제)
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

            Stmt::InlineAnchor { name, body, .. } => {
                let func = self.current_fn.unwrap();
                let anchor_bb = self
                    .context
                    .append_basic_block(func, &format!("anchor_{}", name));
                let recovery_bb = self
                    .context
                    .append_basic_block(func, &format!("recover_{}", name));
                let merge_bb = self
                    .context
                    .append_basic_block(func, &format!("after_{}", name));

                // yield 슬롯 (i64 사용 — 범용)
                let yield_alloca = self.build_alloca(&format!("{}_yield", name), &Ty::I64);
                self.builder
                    .build_store(yield_alloca, self.i64_type().const_int(0, false))
                    .unwrap();
                let kill_count_alloca =
                    self.build_alloca(&format!("{}_kill_count", name), &Ty::I64);
                self.builder
                    .build_store(kill_count_alloca, self.i64_type().const_int(0, false))
                    .unwrap();

                self.builder.build_unconditional_branch(anchor_bb).unwrap();
                self.builder.position_at_end(anchor_bb);

                // 스택에 복구/yield 정보 push
                self.recovery_stack.push(recovery_bb);
                self.kill_cleanup_depth_stack
                    .push(self.vault_scope_stack.len());
                self.yield_slot.push(yield_alloca);
                self.yield_merge_bb.push(merge_bb);
                self.kill_count_slot.push(kill_count_alloca);

                let inline_exp_vaults = self.save_vault_count(name);

                self.compile_stmts(body, params);

                // 정상 종료 시 merge로
                if self.no_terminator() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }

                // 복구 블록 — Kill이 여기로 점프 + vault assert 검증
                self.builder.position_at_end(recovery_bb);
                self.emit_recovery_vault_assert(merge_bb, inline_exp_vaults, name);

                // 스택 pop
                self.recovery_stack.pop();
                self.kill_cleanup_depth_stack.pop();
                self.yield_slot.pop();
                self.yield_merge_bb.pop();
                self.kill_count_slot.pop();

                self.builder.position_at_end(merge_bb);
            }

            Stmt::Match { expr, arms } => {
                let func = self.current_fn.unwrap();
                let val = self.compile_expr(expr, params);
                let expr_ty = self.guess_expr_ty(expr, params);
                let merge_bb = self.context.append_basic_block(func, "match_merge");

                if let Ty::Enum(ref ename) = expr_ty {
                    // ── enum match: LLVM switch on tag ──
                    let enum_struct = val.into_struct_value();
                    let tag = self
                        .builder
                        .build_extract_value(enum_struct, 0, "enum_tag")
                        .unwrap()
                        .into_int_value();

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
                                if let Some(tags) = self.enum_variant_tags.get(ename) {
                                    if let Some(&tv) = tags.get(variant) {
                                        cases.push((
                                            self.context.i32_type().const_int(tv as u64, false),
                                            arm_bbs[i],
                                        ));
                                    }
                                }
                            }
                            Pattern::Wildcard => {
                                wildcard_idx = Some(i);
                            }
                            _ => {}
                        }
                    }

                    // Emit switch (tag → arm block)
                    let actual_default = wildcard_idx.map(|i| arm_bbs[i]).unwrap_or(default_bb);
                    self.builder
                        .build_switch(tag, actual_default, &cases)
                        .unwrap();

                    // Compile each arm
                    let ename_clone = ename.clone();
                    for (i, arm) in arms.iter().enumerate() {
                        self.builder.position_at_end(arm_bbs[i]);

                        // Extract payload binding if needed
                        if let Pattern::EnumVariant {
                            variant,
                            binding: Some(bind_name),
                            ..
                        } = &arm.pattern
                        {
                            if let Some(variants) = self.enum_defs.get(&ename_clone).cloned() {
                                if let Some(v) = variants.iter().find(|v| v.name == *variant) {
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
                                            .build_load(payload_llvm_ty, payload_ptr, bind_name)
                                            .unwrap();
                                        let bind_alloca = self.build_alloca(bind_name, payload_ty);
                                        self.builder.build_store(bind_alloca, payload_val).unwrap();
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
                            _ => {
                                self.builder.build_unconditional_branch(next_bb).unwrap();
                            }
                        }

                        self.builder.position_at_end(arm_bb);
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

    // ── printf 헬퍼 ──

    fn emit_print(&mut self, val: BasicValueEnum<'ctx>, ty: Option<&Ty>) {
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

    // ── 표현식 ──

    fn compile_expr(&mut self, expr: &Expr, params: &[Param]) -> BasicValueEnum<'ctx> {
        match expr {
            Expr::IntLit(n) => self.i64_type().const_int(*n as u64, true).into(),
            Expr::FloatLit(f) => self.f64_type().const_float(*f).into(),
            Expr::StringLit(s) => self.global_string_ptr(s, "str").into(),
            Expr::Bool(b) => self
                .bool_type()
                .const_int(if *b { 1 } else { 0 }, false)
                .into(),
            Expr::Ident(name) => {
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
                    .build_call(func, &compiled_args, &format!("{}_ret", name))
                    .unwrap();
                call_site
                    .try_as_basic_value()
                    .basic()
                    .unwrap_or_else(|| self.i64_type().const_int(0, false).into())
            }
            Expr::MethodCall { base, method, args } => {
                // mod.func() 호출: base가 모듈 이름인 경우
                if let Expr::Ident(base_name) = base.as_ref() {
                    if self.module_names.contains(base_name.as_str()) {
                        let qualified = format!("{}_{}", base_name, method);
                        if let Some(&func) = self.functions.get(&qualified) {
                            let compiled_args: Vec<BasicMetadataValueEnum> = args
                                .iter()
                                .map(|a| self.compile_expr(a, params).into())
                                .collect();
                            let call_site = self
                                .builder
                                .build_call(func, &compiled_args, &format!("{}_ret", qualified))
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
                            .build_call(func, &compiled_args, &format!("{}_ret", fn_name))
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
                let arr_name = if let Expr::Ident(n) = array.as_ref() {
                    Some(n.clone())
                } else {
                    None
                };
                let data_ptr = self.compile_expr(array, params).into_pointer_value();
                let idx = self.compile_expr(index, params).into_int_value();
                let display_name = arr_name.as_deref().unwrap_or("<expr>");
                if let Some(ref n) = arr_name {
                    if let Some(&arr_len) = self.array_lengths.get(n) {
                        self.emit_bounds_check(idx, arr_len, n);
                    } else {
                        // 길이를 모르는 배열: 음수 인덱스만 검사
                        self.emit_negative_index_check(idx, display_name);
                    }
                } else {
                    // 이름 없는 배열 표현식: 음수 인덱스 검사
                    self.emit_negative_index_check(idx, display_name);
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
                if let Some(defs) = self.struct_defs.get(name).cloned() {
                    for (idx, df) in defs.iter().enumerate() {
                        let value = if let Some((_, expr)) =
                            fields.iter().find(|(fname, _)| *fname == df.name)
                        {
                            let v = self.compile_expr(expr, params);
                            self.coerce_to_ty(v, &df.ty)
                        } else {
                            self.ty_to_basic(&df.ty).const_zero()
                        };
                        agg = self
                            .builder
                            .build_insert_value(agg, value, idx as u32, "struct_set")
                            .unwrap()
                            .into_struct_value();
                    }
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

                // 현재 상태 저장
                let saved_fn = self.current_fn;
                let saved_vars = self.variables.clone();
                let saved_var_types = self.var_types.clone();
                let saved_bb = self.builder.get_insert_block();

                // 클로저 함수 본문 빌드
                let entry_bb = self.context.append_basic_block(cl_fn, "entry");
                self.builder.position_at_end(entry_bb);
                self.current_fn = Some(cl_fn);
                self.variables = HashMap::new();
                self.var_types = HashMap::new();

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

                // 상태 복원
                self.current_fn = saved_fn;
                self.variables = saved_vars;
                self.var_types = saved_var_types;
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

    fn elem_llvm_type(&self, ty: &Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        self.ty_to_basic(ty)
    }

    fn guess_var_ty(&self, name: &str, params: &[Param]) -> Ty {
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

    fn guess_expr_ty(&self, expr: &Expr, params: &[Param]) -> Ty {
        match expr {
            Expr::IntLit(_) => Ty::Int,
            Expr::FloatLit(_) => Ty::Float,
            Expr::StringLit(_) => Ty::String,
            Expr::Bool(_) => Ty::Bool,
            Expr::Ident(name) => self.guess_var_ty(name, params),
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
                .and_then(|opt| opt.clone())
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
                        .and_then(|opt| opt.clone())
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

    // ── 출력 ──

    pub fn get_ir_string(&self) -> String {
        self.module.print_to_string().to_string()
    }

    pub fn print_ir(&self) {
        println!("{}", self.module.print_to_string().to_string());
    }

    pub fn run_i64_function(&self, name: &str) -> Result<i64, String> {
        let func = self
            .module
            .get_function(name)
            .ok_or_else(|| format!("function '{}' not found", name))?;

        let engine: ExecutionEngine<'ctx> = self
            .module
            .create_jit_execution_engine(self.opt_level)
            .map_err(|e| format!("failed to create JIT engine: {}", e))?;

        let ret = unsafe { engine.run_function(func, &[]) };
        let value = ret.as_int(false) as i64;
        std::mem::forget(engine);
        Ok(value)
    }

    pub fn run_f64_function(&self, name: &str) -> Result<f64, String> {
        let func = self
            .module
            .get_function(name)
            .ok_or_else(|| format!("function '{}' not found", name))?;

        let engine: ExecutionEngine<'ctx> = self
            .module
            .create_jit_execution_engine(self.opt_level)
            .map_err(|e| format!("failed to create JIT engine: {}", e))?;

        let ret = unsafe { engine.run_function(func, &[]) };
        let value = ret.as_float(&self.f64_type());
        std::mem::forget(engine);
        Ok(value)
    }

    pub fn get_function_return_ty(&self, name: &str) -> Option<Ty> {
        self.fn_return_tys.get(name).and_then(|opt| opt.clone())
    }
}

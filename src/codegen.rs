use crate::ast::*;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::StructType;
use inkwell::values::{FunctionValue, PointerValue};
use inkwell::OptimizationLevel;
use std::collections::HashMap;

#[path = "codegen/checks.rs"]
mod checks;
#[path = "codegen/exprs.rs"]
mod exprs;
#[path = "codegen/jit.rs"]
mod jit;
#[path = "codegen/ops.rs"]
mod ops;
#[path = "codegen/program.rs"]
mod program;
#[path = "codegen/runtime.rs"]
mod runtime;
#[path = "codegen/stmts.rs"]
mod stmts;
#[path = "codegen/types.rs"]
mod types;
#[path = "codegen/vars.rs"]
mod vars;

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

    /// 복구 후 재시작할 앵커 루프 헤더 블록 스택 (recovery_stack과 1:1)
    /// Kill → recovery_bb → emit_recovery_vault_assert → restart_bb (루프 재진입)
    anchor_restart_bb_stack: Vec<BasicBlock<'ctx>>,

    /// recovery_stack과 1:1 대응되는 Vault cleanup depth
    kill_cleanup_depth_stack: Vec<usize>,

    /// yield 값을 저장할 alloca 스택 (anchor yield)
    yield_slot: Vec<PointerValue<'ctx>>,

    /// yield 후 점프할 블록 스택
    yield_merge_bb: Vec<BasicBlock<'ctx>>,

    /// 앵커별 Kill 발생 횟수 슬롯
    kill_count_slot: Vec<PointerValue<'ctx>>,

    /// catch 블록 per-anchor (None = catch 없음) — Kill 라우팅용
    catch_bb_stack: Vec<Option<inkwell::basic_block::BasicBlock<'ctx>>>,
    /// catch 파라미터 메시지 슬롯 per-anchor (i8* alloca)
    catch_msg_slot_stack: Vec<Option<PointerValue<'ctx>>>,

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
            anchor_restart_bb_stack: Vec::new(),
            kill_cleanup_depth_stack: Vec::new(),
            yield_slot: Vec::new(),
            yield_merge_bb: Vec::new(),
            kill_count_slot: Vec::new(),
            catch_bb_stack: Vec::new(),
            catch_msg_slot_stack: Vec::new(),
            vault_live_count: None,
            current_fn: None,
            debug_mode: true,
            opt_level: OptimizationLevel::None,
            string_temp_stack: Vec::new(),
            closure_counter: 0,
            module_names: std::collections::HashSet::new(),
        }
    }
}

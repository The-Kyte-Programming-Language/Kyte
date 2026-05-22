use crate::ast::*;
use std::collections::{HashMap, HashSet};

#[path = "analyzer/control_flow.rs"]
mod control_flow;
#[path = "analyzer/diagnostics.rs"]
mod diagnostics;
#[path = "analyzer/infer.rs"]
mod infer;
#[path = "analyzer/scope.rs"]
mod scope;
#[path = "analyzer/shared.rs"]
mod shared;
#[path = "analyzer/stmt.rs"]
mod stmt;

pub use shared::{CompileError, Severity};
use shared::{FnSig, VarInfo};

pub struct Analyzer {
    errors: Vec<CompileError>,
    functions: HashMap<String, FnSig>,
    structs: HashMap<String, Vec<StructField>>,
    enums: HashMap<String, Vec<crate::ast::EnumVariant>>,
    source_lines: Vec<String>,
    current_span: Span,
    in_anchor: bool,
    /// 사용된 변수 추적 (unused variable 경고용)
    used_vars: HashSet<String>,
    /// 선언된 변수 (이름, span)
    declared_vars: Vec<(String, Span)>,
    /// 분석 설정 (A05)
    config: AnalyzerConfig,
    /// 최상위 const 선언 (전역 상수)
    global_consts: HashMap<String, VarInfo>,
    /// 등록된 모듈 이름 (mod call 해석용)
    module_names: HashSet<String>,
}

/// Analyzer 동작을 제어하는 설정 구조체 (A05)
#[derive(Clone, Debug, Default)]
pub struct AnalyzerConfig {
    /// 모든 경고 활성화 (--Wall)
    pub wall: bool,
    /// 경고를 오류로 처리 (--Werror)
    pub werror: bool,
    /// 미사용 변수 경고 비활성화 (--no-unused)
    pub no_unused: bool,
}

impl Analyzer {
    pub fn analyze(program: &Program, source: &str) -> Vec<CompileError> {
        Self::analyze_with_config(program, source, AnalyzerConfig::default())
    }

    pub fn analyze_with_config(
        program: &Program,
        source: &str,
        config: AnalyzerConfig,
    ) -> Vec<CompileError> {
        let source_lines: Vec<String> = source.lines().map(String::from).collect();
        let mut a = Analyzer {
            errors: Vec::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            source_lines,
            current_span: Span {
                line: 0,
                col: 0,
                len: 1,
            },
            in_anchor: false,
            used_vars: HashSet::new(),
            declared_vars: Vec::new(),
            config,
            global_consts: HashMap::new(),
            module_names: HashSet::new(),
        };

        // 0: verify top-level main anchor existence/uniqueness
        let mut main_count = 0usize;
        let mut first_item_span: Option<Span> = None;
        for (item, item_span) in &program.items {
            if first_item_span.is_none() {
                first_item_span = Some(*item_span);
            }
            if let TopLevel::Anchor {
                kind: AnchorKind::Main,
                ..
            } = item
            {
                main_count += 1;
            }
            if let TopLevel::Anchor {
                name,
                kind: AnchorKind::Main,
                ..
            } = item
            {
                if name != "main" {
                    a.current_span = *item_span;
                    a.err(
                        "E022",
                        format!(
                            "Main anchor name must be '@main(main)', got '@{}(main)'",
                            name
                        ),
                        "Rename the anchor to @main(main)".to_string(),
                    );
                }
            }
        }
        if main_count == 0 {
            a.current_span = first_item_span.unwrap_or(Span {
                line: 1,
                col: 0,
                len: 1,
            });
            a.err(
                "E018",
                "Missing @...(main) anchor".to_string(),
                "Add a top-level main anchor, e.g. @main(main)".to_string(),
            );
        } else if main_count > 1 {
            a.current_span = first_item_span.unwrap_or(Span {
                line: 1,
                col: 0,
                len: 1,
            });
            a.err(
                "E019",
                format!("Multiple main anchors found ({})", main_count),
                "Keep exactly one top-level @...(main) anchor".to_string(),
            );
        }

        // 1: collect function signatures + struct/enum definitions + duplicate check
        for (item, item_span) in &program.items {
            if let TopLevel::Struct { name, fields } = item {
                if a.structs.contains_key(name) {
                    a.current_span = *item_span;
                    a.err(
                        "E025",
                        format!("Duplicate struct '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name),
                    );
                } else {
                    a.structs.insert(name.clone(), fields.clone());
                }
            }
            if let TopLevel::Enum { name, variants } = item {
                if a.enums.contains_key(name) {
                    a.current_span = *item_span;
                    a.err(
                        "E025",
                        format!("Duplicate enum '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name),
                    );
                } else {
                    a.enums.insert(name.clone(), variants.clone());
                }
            }
            if let TopLevel::Function {
                name,
                params,
                return_ty,
                ..
            } = item
            {
                let sig = FnSig {
                    params: params.iter().map(|p| p.ty.clone()).collect(),
                    return_ty: return_ty.clone(),
                };
                if a.functions.contains_key(name) {
                    a.current_span = *item_span;
                    a.err(
                        "E001",
                        format!("Duplicate function '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name),
                    );
                } else {
                    a.functions.insert(name.clone(), sig);
                }
            }
            if let TopLevel::ExternFn {
                name,
                params,
                return_ty,
            } = item
            {
                let sig = FnSig {
                    params: params.iter().map(|p| p.ty.clone()).collect(),
                    return_ty: return_ty.clone(),
                };
                if a.functions.contains_key(name) {
                    a.current_span = *item_span;
                    a.err(
                        "E001",
                        format!("Duplicate function '{}'", name),
                        format!("Remove or rename one of the '{}' definitions", name),
                    );
                } else {
                    a.functions.insert(name.clone(), sig);
                }
            }
            // Impl 블록의 메서드를 TraitName_TypeName_method 형식으로 등록
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
                        let sig = FnSig {
                            params: params.iter().map(|p| p.ty.clone()).collect(),
                            return_ty: return_ty.clone(),
                        };
                        a.functions.insert(qualified, sig);
                    }
                }
            }
            // Module 내부의 함수를 modname_funcname 형식으로 등록
            if let TopLevel::Module {
                name: modname,
                items: mod_items,
            } = item
            {
                a.module_names.insert(modname.clone());
                for (mod_tl, _) in mod_items {
                    if let TopLevel::Function {
                        name: fn_name,
                        params,
                        return_ty,
                        ..
                    } = mod_tl
                    {
                        let qualified = format!("{}_{}", modname, fn_name);
                        let sig = FnSig {
                            params: params.iter().map(|p| p.ty.clone()).collect(),
                            return_ty: return_ty.clone(),
                        };
                        a.functions.insert(qualified, sig);
                    }
                }
            }
            // 최상위 const 선언을 global_consts에 등록
            if let TopLevel::ConstDecl { ty, name, value } = item {
                let effective_ty = if *ty == Ty::Auto {
                    // 간단한 추론: IntLit → Int, FloatLit → Float, StringLit → String, Bool → Bool
                    match value {
                        Expr::IntLit(_) => Ty::Int,
                        Expr::FloatLit(_) => Ty::Float,
                        Expr::StringLit(_) => Ty::String,
                        Expr::Bool(_) => Ty::Bool,
                        _ => Ty::Int,
                    }
                } else {
                    ty.clone()
                };
                a.global_consts.insert(
                    name.clone(),
                    VarInfo {
                        ty: effective_ty,
                        is_vault: false,
                    },
                );
            }
        }

        // H06: 순환 참조 구조체 감지
        for (item, item_span) in &program.items {
            if let TopLevel::Struct { name, .. } = item {
                let mut visiting = HashSet::new();
                if a.check_circular_struct(name, &mut visiting) {
                    a.current_span = *item_span;
                    a.err(
                        "E034",
                        format!("Circular reference detected in struct '{}'", name),
                        "Use a pointer/Vault or break the cycle".to_string(),
                    );
                }
            }
        }

        // 2: analyze bodies
        for (item, item_span) in &program.items {
            match item {
                TopLevel::Anchor { .. } => {
                    a.check_anchor(item, *item_span, &HashMap::new());
                }
                TopLevel::Function {
                    name,
                    params,
                    return_ty,
                    body,
                    type_params,
                    ..
                } => {
                    // 제네릭 함수는 body 분석 건너뜀 — 전문화(specialization)시 타입이 결정됨
                    if !type_params.is_empty() {
                        continue;
                    }
                    let mut scope: HashMap<String, VarInfo> = HashMap::new();
                    for p in params {
                        scope.insert(
                            p.name.clone(),
                            VarInfo {
                                ty: p.ty.clone(),
                                is_vault: false,
                            },
                        );
                    }
                    a.check_stmts(body, &mut scope, return_ty.as_ref(), name);
                    // H04: 반환값이 있는 함수의 모든 경로에 return이 있는지 검사
                    if return_ty.is_some() && !Self::block_definitely_exits(body) {
                        a.current_span = *item_span;
                        a.err(
                            "E033",
                            format!("Function '{}' may not return a value on all paths", name),
                            "Ensure every code path has a return statement".to_string(),
                        );
                    }
                    // H07: 미사용 변수 경고
                    for p in params {
                        a.declared_vars.push((p.name.clone(), *item_span));
                    }
                }
                TopLevel::Struct { .. } => {}
                TopLevel::Enum { .. } => {}
                TopLevel::Trait { .. } => {} // trait은 선언만, 본문 없음
                TopLevel::Impl {
                    trait_name,
                    target_ty,
                    methods,
                } => {
                    // impl 메서드도 일반 함수처럼 분석
                    for (method_tl, _method_span) in methods {
                        if let TopLevel::Function {
                            name: mname,
                            params,
                            return_ty,
                            body,
                            ..
                        } = method_tl
                        {
                            let qualified = format!("{}_{}_{}", trait_name, target_ty, mname);
                            let mut scope: HashMap<String, VarInfo> = HashMap::new();
                            for p in params {
                                scope.insert(
                                    p.name.clone(),
                                    VarInfo {
                                        ty: p.ty.clone(),
                                        is_vault: false,
                                    },
                                );
                            }
                            a.check_stmts(body, &mut scope, return_ty.as_ref(), &qualified);
                        }
                    }
                }
                TopLevel::Module {
                    name: modname,
                    items: mod_items,
                } => {
                    // module 내부 함수 분석
                    for (mod_tl, _mod_span) in mod_items {
                        if let TopLevel::Function {
                            name: fn_name,
                            params,
                            return_ty,
                            body,
                            ..
                        } = mod_tl
                        {
                            let qualified = format!("{}_{}", modname, fn_name);
                            let mut scope: HashMap<String, VarInfo> = HashMap::new();
                            for p in params {
                                scope.insert(
                                    p.name.clone(),
                                    VarInfo {
                                        ty: p.ty.clone(),
                                        is_vault: false,
                                    },
                                );
                            }
                            a.check_stmts(body, &mut scope, return_ty.as_ref(), &qualified);
                        }
                    }
                }
                TopLevel::ConstDecl { .. } => {} // 최상위 const는 값 체크 생략 (간단히 통과)
                TopLevel::ExternFn { .. } => {} // extern declarations handled in codegen
            }
        }

        a.errors
    }
}

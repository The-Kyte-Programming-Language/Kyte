use std::collections::{HashMap, HashSet};

use crate::analyzer::shared::{is_integer_ty, is_numeric_ty, ty_name, types_compatible, VarInfo};
use crate::analyzer::Analyzer;
use crate::ast::*;

pub(super) fn int_range(ty: &Ty) -> Option<(i128, i128)> {
    match ty {
        Ty::I8 => Some((-128, 127)),
        Ty::I16 => Some((-32768, 32767)),
        Ty::I32 => Some((-2147483648, 2147483647)),
        Ty::I64 | Ty::Int => Some((-9223372036854775808, 9223372036854775807)),
        Ty::U8 => Some((0, 255)),
        Ty::U16 => Some((0, 65535)),
        Ty::U32 => Some((0, 4294967295)),
        Ty::U64 => Some((0, 18446744073709551615)),
        _ => None,
    }
}

pub(super) fn const_int_value(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::IntLit(n) => Some(*n as i128),
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr,
        } => const_int_value(expr).map(|v| -v),
        _ => None,
    }
}

impl Analyzer {
    /// H03: 정수 리터럴이 대상 타입 범위를 초과하는지 검사
    pub(super) fn check_int_range(&mut self, ty: &Ty, value: &Expr) {
        if let Some((lo, hi)) = int_range(ty) {
            if let Some(val) = const_int_value(value) {
                if val < lo || val > hi {
                    self.warn(
                        "W002",
                        format!(
                            "Integer literal {} overflows type {} (range {}..{})",
                            val,
                            ty_name(ty),
                            lo,
                            hi
                        ),
                        format!("Use a larger type or a value within {}..{}", lo, hi),
                    );
                }
            }
        }
    }

    /// H06: 순환 참조 구조체 감지
    pub(super) fn check_circular_struct(&self, name: &str, visiting: &mut HashSet<String>) -> bool {
        if !visiting.insert(name.to_string()) {
            return true; // cycle detected
        }
        if let Some(fields) = self.structs.get(name) {
            for field in fields {
                if let Ty::Struct(ref sname) = field.ty {
                    if self.check_circular_struct(sname, visiting) {
                        return true;
                    }
                }
            }
        }
        visiting.remove(name);
        false
    }

    pub(super) fn infer_expr(
        &mut self,
        expr: &Expr,
        scope: &HashMap<String, VarInfo>,
    ) -> Option<Ty> {
        match expr {
            Expr::IntLit(_) => Some(Ty::Int),
            Expr::FloatLit(_) => Some(Ty::Float),
            Expr::StringLit(_) => Some(Ty::String),
            Expr::Bool(_) => Some(Ty::Bool),

            Expr::Ident(name, span) => {
                self.current_span = *span;
                self.used_vars.insert(name.clone());
                if let Some(info) = scope.get(name) {
                    Some(info.ty.clone())
                } else {
                    self.err_undeclared_var(name, scope);
                    None
                }
            }

            Expr::UnaryOp { op, expr } => {
                let inner = self.infer_expr(expr, scope);
                match op {
                    UnaryOpKind::Neg => {
                        if let Some(ref t) = inner {
                            if !is_numeric_ty(t) {
                                self.err(
                                    "E007",
                                    format!("Cannot negate type {}", ty_name(t)),
                                    "Negation (-) only works on numeric types".into(),
                                );
                            }
                        }
                        inner
                    }
                    UnaryOpKind::Not => {
                        if let Some(ref t) = inner {
                            if *t != Ty::Bool {
                                self.err(
                                    "E007",
                                    format!("Cannot apply '!' to type {}", ty_name(t)),
                                    "Logical not (!) only works on bool values".into(),
                                );
                            }
                        }
                        Some(Ty::Bool)
                    }
                }
            }

            Expr::BinOp { left, op, right } => {
                let lt = self.infer_expr(left, scope);
                let rt = self.infer_expr(right, scope);

                match op {
                    BinOpKind::Add
                    | BinOpKind::Sub
                    | BinOpKind::Mul
                    | BinOpKind::Div
                    | BinOpKind::Mod => {
                        if matches!(op, BinOpKind::Add)
                            && matches!((&lt, &rt), (Some(Ty::String), _) | (_, Some(Ty::String)))
                        {
                            // string + non-string -> auto-concat (handled in codegen)
                            return Some(Ty::String);
                        }
                        // string -, *, /, % are invalid
                        if let (Some(ref l), Some(ref _r)) = (&lt, &rt) {
                            if *l == Ty::String {
                                self.err(
                                    "E008",
                                    format!(
                                        "Cannot use '{}' on string type",
                                        match op {
                                            BinOpKind::Sub => "-",
                                            BinOpKind::Mul => "*",
                                            BinOpKind::Div => "/",
                                            BinOpKind::Mod => "%",
                                            _ => "?",
                                        }
                                    ),
                                    "Only '+' is allowed for string concatenation".into(),
                                );
                                return lt;
                            }
                        }
                        if let (Some(ref l), Some(ref r)) = (&lt, &rt) {
                            if !types_compatible(l, r) && !types_compatible(r, l) {
                                self.err("E008",
                                    format!("Arithmetic type mismatch \u{2014} {} vs {}", ty_name(l), ty_name(r)),
                                    "Both sides of an arithmetic operation must be the same numeric type".into());
                            }
                            if !is_numeric_ty(l) {
                                self.err("E008",
                                    format!("Arithmetic on non-numeric type {}", ty_name(l)),
                                    "Arithmetic operators (+, -, *, /, %) only work on numeric types".into());
                            }
                        }
                        lt
                    }
                    BinOpKind::Lt
                    | BinOpKind::Gt
                    | BinOpKind::Le
                    | BinOpKind::Ge
                    | BinOpKind::Eq
                    | BinOpKind::Neq => {
                        if let (Some(ref l), Some(ref r)) = (&lt, &rt) {
                            if !types_compatible(l, r) && !types_compatible(r, l) {
                                self.err(
                                    "E009",
                                    format!(
                                        "Comparison type mismatch \u{2014} {} vs {}",
                                        ty_name(l),
                                        ty_name(r)
                                    ),
                                    "Both sides of a comparison must be the same type".into(),
                                );
                            }
                        }
                        Some(Ty::Bool)
                    }
                    BinOpKind::And | BinOpKind::Or => {
                        if let Some(ref l) = lt {
                            if *l != Ty::Bool {
                                self.err(
                                    "E010",
                                    format!("Logical operator requires bool, got {}", ty_name(l)),
                                    "Use a comparison (e.g. x > 0) to produce a bool".into(),
                                );
                            }
                        }
                        if let Some(ref r) = rt {
                            if *r != Ty::Bool {
                                self.err(
                                    "E010",
                                    format!("Logical operator requires bool, got {}", ty_name(r)),
                                    "Use a comparison (e.g. x > 0) to produce a bool".into(),
                                );
                            }
                        }
                        Some(Ty::Bool)
                    }
                }
            }

            Expr::Call { name, args, span } => {
                self.current_span = *span;
                // len() 빌트인
                if name == "len" {
                    if args.len() != 1 {
                        self.err(
                            "E011",
                            format!("'len' expects 1 argument, got {}", args.len()),
                            "Usage: len(array)".into(),
                        );
                    } else {
                        let arg_ty = self.infer_expr(&args[0], scope);
                        if let Some(ref at) = arg_ty {
                            if !matches!(at, Ty::Array(_)) {
                                self.err(
                                    "E015",
                                    format!("'len' expects an array, got {}", ty_name(at)),
                                    "Pass an array variable to len()".into(),
                                );
                            }
                        }
                    }
                    return Some(Ty::Int);
                }

                // print() builtin — reachable via >> pipe operator
                if name == "print" {
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    return Some(Ty::Int);
                }

                if let Some(sig) = self.functions.get(name).cloned() {
                    if args.len() != sig.params.len() {
                        self.err(
                            "E011",
                            format!(
                                "'{}' expects {} argument(s), got {}",
                                name,
                                sig.params.len(),
                                args.len()
                            ),
                            format!(
                                "Signature: {}({})",
                                name,
                                sig.params
                                    .iter()
                                    .map(ty_name)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = self.infer_expr(arg, scope);
                        if let (Some(ref at), Some(expected)) = (&arg_ty, sig.params.get(i)) {
                            if !types_compatible(expected, at) {
                                self.err(
                                    "E012",
                                    format!(
                                        "Arg {} of '{}' \u{2014} expected {}, got {}",
                                        i + 1,
                                        name,
                                        ty_name(expected),
                                        ty_name(at)
                                    ),
                                    format!(
                                        "Pass a {} value as argument {}",
                                        ty_name(expected),
                                        i + 1
                                    ),
                                );
                            }
                        }
                    }
                    sig.return_ty
                } else if scope.contains_key(name) {
                    // 스코프에 있는 변수 → 클로저/함수 포인터 호출
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    // 반환 타입 추론: Ty::Fn의 ret 타입이거나 Int로 fallback
                    if let Some(var_info) = scope.get(name) {
                        if let Ty::Fn(_, ret_ty) = &var_info.ty {
                            ret_ty.as_ref().map(|t| *t.clone())
                        } else {
                            Some(Ty::Int)
                        }
                    } else {
                        Some(Ty::Int)
                    }
                } else {
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    self.err_undeclared_fn(name);
                    None
                }
            }

            Expr::MethodCall { base, method, args } => {
                // mod.func() 호출 처리
                if let Expr::Ident(base_name, _) = base.as_ref() {
                    if self.module_names.contains(base_name.as_str()) {
                        let qualified = format!("{}_{}", base_name, method);
                        if let Some(sig) = self.functions.get(&qualified).cloned() {
                            if args.len() != sig.params.len() {
                                self.err(
                                    "E011",
                                    format!(
                                        "'{}' expects {} argument(s), got {}",
                                        qualified,
                                        sig.params.len(),
                                        args.len()
                                    ),
                                    format!("Call as {}.{}(...)", base_name, method),
                                );
                            }
                            for (i, arg) in args.iter().enumerate() {
                                let arg_ty = self.infer_expr(arg, scope);
                                if let (Some(ref at), Some(expected)) = (&arg_ty, sig.params.get(i))
                                {
                                    if !types_compatible(expected, at) {
                                        self.err(
                                            "E012",
                                            format!(
                                                "Arg {} of '{}' — expected {}, got {}",
                                                i + 1,
                                                qualified,
                                                ty_name(expected),
                                                ty_name(at)
                                            ),
                                            format!(
                                                "Pass a {} value as argument {}",
                                                ty_name(expected),
                                                i + 1
                                            ),
                                        );
                                    }
                                }
                            }
                            return sig.return_ty;
                        } else {
                            for arg in args {
                                self.infer_expr(arg, scope);
                            }
                            self.err(
                                "E013",
                                format!("Undeclared function '{}.{}'", base_name, method),
                                format!(
                                    "Define it with mod {} {{ fn {}(...) {{ ... }} }}",
                                    base_name, method
                                ),
                            );
                            return None;
                        }
                    }
                }
                let base_ty = self.infer_expr(base, scope);
                let sname = if let Some(Ty::Struct(n)) = base_ty.clone() {
                    n
                } else {
                    self.err(
                        "E028",
                        format!("Cannot call method '{}' on non-struct value", method),
                        "Method call requires a struct value (e.g. user.method()) or a module (e.g. mod.func())".to_string(),
                    );
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    return None;
                };

                let fn_name = format!("{}.{}", sname, method);
                if let Some(sig) = self.functions.get(&fn_name).cloned() {
                    // method는 self 파라미터(자동 삽입) + 사용자 인자들
                    let expected_user_args = sig.params.len().saturating_sub(1);
                    if args.len() != expected_user_args {
                        self.err(
                            "E011",
                            format!(
                                "'{}' expects {} argument(s), got {}",
                                fn_name,
                                expected_user_args,
                                args.len()
                            ),
                            format!("Method signature: {}(...)", fn_name),
                        );
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = self.infer_expr(arg, scope);
                        if let (Some(ref at), Some(expected)) = (&arg_ty, sig.params.get(i + 1)) {
                            if !types_compatible(expected, at) {
                                self.err(
                                    "E012",
                                    format!(
                                        "Arg {} of '{}' — expected {}, got {}",
                                        i + 1,
                                        fn_name,
                                        ty_name(expected),
                                        ty_name(at)
                                    ),
                                    format!(
                                        "Pass a {} value as argument {}",
                                        ty_name(expected),
                                        i + 1
                                    ),
                                );
                            }
                        }
                    }
                    sig.return_ty
                } else {
                    for arg in args {
                        self.infer_expr(arg, scope);
                    }
                    self.err(
                        "E013",
                        format!("Undeclared method '{}.{}'", sname, method),
                        format!("Define it with fn {}.{}(...) {{ ... }}", sname, method),
                    );
                    None
                }
            }

            Expr::ArrayLit(elems) => {
                if elems.is_empty() {
                    self.err(
                        "E016",
                        "Empty array literal \u{2014} cannot infer element type".to_string(),
                        "Provide at least one element, e.g. [0]".into(),
                    );
                    return None;
                }
                let first_ty = self.infer_expr(&elems[0], scope);
                for (i, elem) in elems.iter().enumerate().skip(1) {
                    let elem_ty = self.infer_expr(elem, scope);
                    if let (Some(ref ft), Some(ref et)) = (&first_ty, &elem_ty) {
                        if ft != et {
                            self.err(
                                "E002",
                                format!(
                                    "Array element {} has type {}, expected {}",
                                    i,
                                    ty_name(et),
                                    ty_name(ft)
                                ),
                                "All array elements must be the same type".into(),
                            );
                        }
                    }
                }
                first_ty.map(|t| Ty::Array(Box::new(t)))
            }

            Expr::Index { array, index } => {
                let arr_ty = self.infer_expr(array, scope);
                let idx_ty = self.infer_expr(index, scope);
                if let Some(ref it) = idx_ty {
                    if *it != Ty::Int {
                        self.err(
                            "E014",
                            format!("Array index must be int, got {}", ty_name(it)),
                            "Use an integer value for array indexing".into(),
                        );
                    }
                }
                if let Some(Ty::Array(inner)) = arr_ty {
                    Some(*inner)
                } else {
                    if let Some(ref t) = arr_ty {
                        self.err(
                            "E015",
                            format!("Cannot index into non-array type {}", ty_name(t)),
                            "Only array types support indexing".into(),
                        );
                    }
                    None
                }
            }

            Expr::Cast {
                expr,
                ty: target_ty,
            } => {
                let src_ty = self.infer_expr(expr, scope);
                if let Some(ref st) = src_ty {
                    let valid = match (st, target_ty) {
                        // numeric ??numeric
                        (s, t) if is_numeric_ty(s) && is_numeric_ty(t) => true,
                        // bool ??int
                        (Ty::Bool, t) if is_integer_ty(t) => true,
                        // int ??bool
                        (s, Ty::Bool) if is_integer_ty(s) => true,
                        _ => false,
                    };
                    if !valid {
                        self.err(
                            "E021",
                            format!("Cannot cast {} to {}", ty_name(st), ty_name(target_ty)),
                            "Only numeric type conversions are allowed with 'as'".into(),
                        );
                    }
                }
                Some(target_ty.clone())
            }
            Expr::StructInit { name, fields } => {
                if let Some(def_fields) = self.structs.get(name).cloned() {
                    for df in &def_fields {
                        if !fields.iter().any(|(fname, _)| fname == &df.name) {
                            self.err(
                                "E029",
                                format!("Missing field '{}' in struct init '{}'", df.name, name),
                                format!("Provide '{}: ...' in {} {{ ... }}", df.name, name),
                            );
                        }
                    }
                    for (fname, fexpr) in fields {
                        if let Some(df) = def_fields.iter().find(|f| f.name == *fname) {
                            let got = self.infer_expr(fexpr, scope);
                            if let Some(gt) = got {
                                if !types_compatible(&df.ty, &gt) {
                                    self.err(
                                        "E002",
                                        format!(
                                            "Type mismatch for field '{}.{}' — expected {}, got {}",
                                            name,
                                            fname,
                                            ty_name(&df.ty),
                                            ty_name(&gt)
                                        ),
                                        format!(
                                            "Field '{}.{}' type is {}",
                                            name,
                                            fname,
                                            ty_name(&df.ty)
                                        ),
                                    );
                                }
                            }
                        } else {
                            self.err(
                                "E027",
                                format!("Struct '{}' has no field '{}'", name, fname),
                                "Check the struct declaration for available fields".to_string(),
                            );
                            self.infer_expr(fexpr, scope);
                        }
                    }
                } else {
                    self.err(
                        "E026",
                        format!("Unknown struct type '{}'", name),
                        format!("Declare 'struct {} {{ ... }}' before using it", name),
                    );
                    for (_, fexpr) in fields {
                        self.infer_expr(fexpr, scope);
                    }
                }
                Some(Ty::Struct(name.clone()))
            }
            Expr::FieldAccess { base, field } => {
                let bt = self.infer_expr(base, scope);
                if let Some(Ty::Struct(sname)) = bt {
                    if let Some(fields) = self.structs.get(&sname) {
                        if let Some(sf) = fields.iter().find(|f| f.name == *field) {
                            Some(sf.ty.clone())
                        } else {
                            self.err(
                                "E027",
                                format!("Struct '{}' has no field '{}'", sname, field),
                                "Check the struct declaration for available fields".to_string(),
                            );
                            None
                        }
                    } else {
                        self.err(
                            "E026",
                            format!("Unknown struct type '{}'", sname),
                            format!("Declare 'struct {} {{ ... }}' before using it", sname),
                        );
                        None
                    }
                } else {
                    if let Some(t) = bt {
                        self.err(
                            "E028",
                            format!(
                                "Cannot access field '{}' on non-struct type {}",
                                field,
                                ty_name(&t)
                            ),
                            "Field access requires a struct value".to_string(),
                        );
                    }
                    None
                }
            }
            Expr::EnumVariant {
                enum_name,
                variant,
                value,
            } => {
                if let Some(variants) = self.enums.get(enum_name).cloned() {
                    if let Some(v) = variants.iter().find(|v| v.name == *variant) {
                        match (&v.ty, value) {
                            (Some(expected_ty), Some(val_expr)) => {
                                let val_ty = self.infer_expr(val_expr, scope);
                                if let Some(vt) = &val_ty {
                                    if !types_compatible(expected_ty, vt) {
                                        self.err(
                                            "E002",
                                            format!(
                                                "Enum variant '{}.{}' expects {}, got {}",
                                                enum_name,
                                                variant,
                                                ty_name(expected_ty),
                                                ty_name(vt)
                                            ),
                                            format!(
                                                "Pass a {} value to {}.{}",
                                                ty_name(expected_ty),
                                                enum_name,
                                                variant
                                            ),
                                        );
                                    }
                                }
                            }
                            (None, Some(val_expr)) => {
                                self.infer_expr(val_expr, scope);
                                self.err(
                                    "E039",
                                    format!(
                                        "Enum variant '{}.{}' takes no payload",
                                        enum_name, variant
                                    ),
                                    format!("Use '{}.{}' without parentheses", enum_name, variant),
                                );
                            }
                            (Some(expected_ty), None) => {
                                self.err(
                                    "E039",
                                    format!(
                                        "Enum variant '{}.{}' requires a {} payload",
                                        enum_name,
                                        variant,
                                        ty_name(expected_ty)
                                    ),
                                    format!("Use '{}.{}(value)'", enum_name, variant),
                                );
                            }
                            (None, None) => {}
                        }
                    } else {
                        self.err(
                            "E037",
                            format!("Enum '{}' has no variant '{}'", enum_name, variant),
                            format!(
                                "Available variants: {}",
                                variants
                                    .iter()
                                    .map(|v| v.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                } else {
                    self.err(
                        "E038",
                        format!("Unknown enum type '{}'", enum_name),
                        format!("Declare 'enum {} {{ ... }}' before using it", enum_name),
                    );
                }
                Some(Ty::Enum(enum_name.clone()))
            }
            Expr::Closure { params, body } => {
                // 클로저 본체 분석 (파라미터 추가)
                let mut closure_scope = scope.clone();
                let param_tys: Vec<Ty> = params
                    .iter()
                    .map(|(_, opt_ty)| opt_ty.clone().unwrap_or(Ty::Auto))
                    .collect();
                for (pname, opt_ty) in params {
                    let ty = opt_ty.clone().unwrap_or(Ty::Auto);
                    closure_scope.insert(
                        pname.clone(),
                        VarInfo {
                            ty,
                            is_vault: false,
                        },
                    );
                }
                let fn_name = "<closure>";
                let ret_ty: Option<Ty> = None;
                self.check_stmts(body, &mut closure_scope, ret_ty.as_ref(), fn_name);
                Some(Ty::Fn(param_tys, None))
            }
            Expr::FStringLit(parts) => {
                for part in parts {
                    if let crate::ast::FStringPart::Expr(e) = part {
                        self.infer_expr(e, scope);
                    }
                }
                Some(Ty::String)
            }
        }
    }
}

use crate::ast::*;
use std::fmt;

const RED: &str = "\x1b[1;31m";
const YELLOW: &str = "\x1b[1;33m";
const CYAN: &str = "\x1b[1;36m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut dp: Vec<usize> = (0..=b_chars.len()).collect();
    for (i, ca) in a_chars.iter().enumerate() {
        let mut prev = dp[0];
        dp[0] = i + 1;
        for (j, cb) in b_chars.iter().enumerate() {
            let tmp = dp[j + 1];
            let cost = if ca == cb { 0 } else { 1 };
            dp[j + 1] = (dp[j + 1] + 1).min(dp[j] + 1).min(prev + cost);
            prev = tmp;
        }
    }
    dp[b_chars.len()]
}

pub fn nearest_name<'a>(target: &str, candidates: impl Iterator<Item = &'a String>) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for c in candidates {
        let d = levenshtein(target, c);
        if d <= 2 {
            match &best {
                Some((best_d, _)) if d >= *best_d => {}
                _ => best = Some((d, c.clone())),
            }
        }
    }
    best.map(|(_, s)| s)
}

#[derive(Clone, Debug)]
pub struct CompileError {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub hint: String,
    pub span: Span,
    pub source_line: String,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (icon, color, label) = match self.severity {
            Severity::Error => ("\u{2718}", RED, "ERROR"),
            Severity::Warning => ("\u{26A0}", YELLOW, "WARN "),
        };
        writeln!(
            f,
            "  {color}{icon} {label} [{code}]{RESET} {BOLD}{msg}{RESET}",
            code = self.code,
            msg = self.message
        )?;
        writeln!(
            f,
            "     {DIM}\u{2500}\u{2192} line {}:{}{RESET}",
            self.span.line,
            self.span.col
        )?;
        let trimmed = self.source_line.trim_end();
        if !trimmed.is_empty() {
            let leading = trimmed.len() - trimmed.trim_start().len();
            writeln!(f, "      {DIM}\u{2502}{RESET}")?;
            writeln!(
                f,
                "  {DIM}{:>3}{RESET} {DIM}\u{2502}{RESET} {}",
                self.span.line,
                trimmed
            )?;
            writeln!(
                f,
                "      {DIM}\u{2502}{RESET} {}{color}{}{RESET}",
                " ".repeat(leading),
                "\u{2500}".repeat(trimmed.trim_start().len())
            )?;
        }
        writeln!(f, "      {CYAN}\u{21B3} hint:{RESET} {DIM}{}{RESET}", self.hint)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FnSig {
    pub(crate) params: Vec<Ty>,
    pub(crate) return_ty: Option<Ty>,
}

#[derive(Clone, Debug)]
pub(crate) struct VarInfo {
    pub(crate) ty: Ty,
    #[allow(dead_code)]
    pub(crate) is_vault: bool,
}

pub fn ty_name(ty: &Ty) -> String {
    match ty {
        Ty::Int => "int".to_string(),
        Ty::Float => "float".to_string(),
        Ty::String => "string".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::I8 => "i8".to_string(),
        Ty::I16 => "i16".to_string(),
        Ty::I32 => "i32".to_string(),
        Ty::I64 => "i64".to_string(),
        Ty::U8 => "u8".to_string(),
        Ty::U16 => "u16".to_string(),
        Ty::U32 => "u32".to_string(),
        Ty::U64 => "u64".to_string(),
        Ty::Array(inner) => format!("{}[]", ty_name(inner)),
        Ty::Struct(name) => name.clone(),
        Ty::Auto => "auto".to_string(),
        Ty::Enum(name) => name.clone(),
        Ty::TypeParam(name) => name.clone(),
        Ty::Fn(params, ret) => {
            let ps: Vec<String> = params.iter().map(ty_name).collect();
            let ret_str = ret
                .as_deref()
                .map(ty_name)
                .unwrap_or_else(|| "void".to_string());
            format!("fn({}) -> {}", ps.join(", "), ret_str)
        }
    }
}

pub fn is_integer_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
    )
}

pub fn is_numeric_ty(ty: &Ty) -> bool {
    is_integer_ty(ty) || matches!(ty, Ty::Float)
}

pub fn types_compatible(expected: &Ty, got: &Ty) -> bool {
    if expected == got {
        return true;
    }
    if *got == Ty::Int && is_integer_ty(expected) {
        return true;
    }
    if (*expected == Ty::Int && *got == Ty::I64) || (*expected == Ty::I64 && *got == Ty::Int) {
        return true;
    }
    false
}

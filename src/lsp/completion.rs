use std::collections::HashSet;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use lsp_types::{CompletionItem, CompletionItemKind, CompletionList, Uri};

use super::imports::{parse_import_path, preprocess_source, uri_to_file_path};
use super::util::ty_str;
use crate::ast::TopLevel;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_completions(uri: &Uri, text: Option<&str>) -> CompletionList {
    let mut items: Vec<CompletionItem> = KEYWORDS
        .iter()
        .map(|&(label, kind, detail)| CompletionItem {
            label: label.into(),
            kind: Some(kind),
            detail: Some(detail.into()),
            ..Default::default()
        })
        .collect();

    // 현재 파일 함수 추가
    if let Some(src) = text {
        let src = src.to_string();
        if let Ok(fns) = catch_unwind(AssertUnwindSafe(|| extract_fn_names(&src))) {
            for (name, sig) in fns {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(sig),
                    ..Default::default()
                });
            }
        }

        // import한 파일들의 함수도 추가
        if let Some(root_path) = uri_to_file_path(uri) {
            let base_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
            let mut seen: HashSet<PathBuf> = HashSet::new();
            if let Ok(canon) = fs::canonicalize(&root_path) {
                seen.insert(canon);
            } else {
                seen.insert(root_path.clone());
            }

            for line in src.lines() {
                if let Some(rel) = parse_import_path(line) {
                    let import_path = base_dir.join(&rel);
                    let resolved =
                        fs::canonicalize(&import_path).unwrap_or_else(|_| import_path.clone());

                    if seen.insert(resolved.clone()) {
                        if let Ok(import_src) = fs::read_to_string(&resolved)
                            .or_else(|_| fs::read_to_string(&import_path))
                        {
                            // import 파일 이름 (detail에 표시)
                            let file_label = rel.clone();
                            if let Ok(fns) =
                                catch_unwind(AssertUnwindSafe(|| extract_fn_names(&import_src)))
                            {
                                for (name, sig) in fns {
                                    items.push(CompletionItem {
                                        label: name,
                                        kind: Some(CompletionItemKind::FUNCTION),
                                        detail: Some(format!("{} [from {}]", sig, file_label)),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    CompletionList {
        is_incomplete: false,
        items,
    }
}

fn extract_fn_names(src: &str) -> Vec<(String, String)> {
    let src = preprocess_source(src);
    let mut lex = Lexer::new(&src);
    let tokens = lex.tokenize();
    let mut par = Parser::new(tokens);
    let ast = par.parse();
    let mut out = Vec::new();
    for (item, _) in &ast.items {
        if let TopLevel::Function {
            name,
            params,
            return_ty,
            ..
        } = item
        {
            let ps: Vec<String> = params
                .iter()
                .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                .collect();
            let ret = return_ty
                .as_ref()
                .map(|t| format!(" -> {}", ty_str(t)))
                .unwrap_or_default();
            out.push((name.clone(), format!("fn({}){}", ps.join(", "), ret)));
        }
    }
    out
}

const KEYWORDS: &[(&str, CompletionItemKind, &str)] = &[
    ("fn", CompletionItemKind::KEYWORD, "Function declaration"),
    (
        "struct",
        CompletionItemKind::KEYWORD,
        "Struct type declaration",
    ),
    (
        "int",
        CompletionItemKind::TYPE_PARAMETER,
        "64-bit signed integer (alias for i64)",
    ),
    ("float", CompletionItemKind::TYPE_PARAMETER, "64-bit float"),
    ("string", CompletionItemKind::TYPE_PARAMETER, "String type"),
    ("bool", CompletionItemKind::TYPE_PARAMETER, "Boolean type"),
    (
        "i8",
        CompletionItemKind::TYPE_PARAMETER,
        "8-bit signed integer",
    ),
    (
        "i16",
        CompletionItemKind::TYPE_PARAMETER,
        "16-bit signed integer",
    ),
    (
        "i32",
        CompletionItemKind::TYPE_PARAMETER,
        "32-bit signed integer",
    ),
    (
        "i64",
        CompletionItemKind::TYPE_PARAMETER,
        "64-bit signed integer",
    ),
    (
        "u8",
        CompletionItemKind::TYPE_PARAMETER,
        "8-bit unsigned integer",
    ),
    (
        "u16",
        CompletionItemKind::TYPE_PARAMETER,
        "16-bit unsigned integer",
    ),
    (
        "u32",
        CompletionItemKind::TYPE_PARAMETER,
        "32-bit unsigned integer",
    ),
    (
        "u64",
        CompletionItemKind::TYPE_PARAMETER,
        "64-bit unsigned integer",
    ),
    ("if", CompletionItemKind::KEYWORD, "Conditional"),
    ("else", CompletionItemKind::KEYWORD, "Alternative branch"),
    ("for", CompletionItemKind::KEYWORD, "Range loop"),
    ("loop", CompletionItemKind::KEYWORD, "Infinite loop"),
    ("while", CompletionItemKind::KEYWORD, "Conditional loop"),
    ("break", CompletionItemKind::KEYWORD, "Exit loop"),
    ("return", CompletionItemKind::KEYWORD, "Return value"),
    (
        "yield",
        CompletionItemKind::KEYWORD,
        "Transfer data out of anchor",
    ),
    (
        "Vault",
        CompletionItemKind::KEYWORD,
        "Managed heap memory (auto-freed at scope exit)",
    ),
    (
        "free",
        CompletionItemKind::FUNCTION,
        "⚠ Deprecated — Vault memory is now auto-freed at scope exit",
    ),
    (
        "print",
        CompletionItemKind::FUNCTION,
        "Print values to stdout",
    ),
    ("as", CompletionItemKind::KEYWORD, "Type cast: expr as type"),
    (
        "import",
        CompletionItemKind::KEYWORD,
        "Import another .ky source file",
    ),
    (
        "Kill",
        CompletionItemKind::KEYWORD,
        "Terminate anchor with recovery",
    ),
    ("Exit", CompletionItemKind::KEYWORD, "Exit program"),
    ("true", CompletionItemKind::CONSTANT, "Boolean true"),
    ("false", CompletionItemKind::CONSTANT, "Boolean false"),
    (
        "auto",
        CompletionItemKind::KEYWORD,
        "Type inference: auto x = expr;",
    ),
    (
        "assert",
        CompletionItemKind::FUNCTION,
        "Assert condition: assert(cond);",
    ),
    ("enum", CompletionItemKind::KEYWORD, "Enum type declaration"),
    (
        "match",
        CompletionItemKind::KEYWORD,
        "Pattern matching: match expr { pat => { ... } }",
    ),
    ("trait", CompletionItemKind::KEYWORD, "Trait declaration"),
    (
        "impl",
        CompletionItemKind::KEYWORD,
        "Trait implementation block",
    ),
    (
        "mod",
        CompletionItemKind::KEYWORD,
        "Module/namespace declaration",
    ),
    (
        "const",
        CompletionItemKind::KEYWORD,
        "Immutable constant variable",
    ),
];

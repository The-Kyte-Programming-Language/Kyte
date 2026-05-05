use std::collections::HashSet;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, InsertTextFormat, Position, Uri,
};

use super::imports::{expand_import_line, parse_import_path, preprocess_source, uri_to_file_path};
use super::util::ty_str;
use crate::ast::TopLevel;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_completions(uri: &Uri, text: Option<&str>, pos: Position) -> CompletionList {
    let src_text = text.unwrap_or("");

    // ── 컨텍스트 감지: 커서 앞 텍스트 ──────────────────────────────────────
    let line_str = src_text.lines().nth(pos.line as usize).unwrap_or("");
    let col = (pos.character as usize).min(line_str.len());
    let before = &line_str[..col];
    let before_trimmed = before.trim_end();

    // `.` 다음: struct field / enum variant 자동완성
    if before_trimmed.ends_with('.') {
        let base = extract_trailing_ident(&before_trimmed[..before_trimmed.len() - 1]);
        if !base.is_empty() {
            if let Some(items) = dot_completions(src_text, &base) {
                return CompletionList {
                    is_incomplete: false,
                    items,
                };
            }
        }
    }

    // ── 일반 자동완성 ─────────────────────────────────────────────────────
    let mut items: Vec<CompletionItem> = Vec::new();

    // 1. 키워드 스니펫
    items.extend(keyword_snippet_items());

    // 2. 현재 파일 함수 + struct + enum + const
    if let Ok(decl_items) = catch_unwind(AssertUnwindSafe(|| extract_decl_completions(src_text))) {
        items.extend(decl_items);
    }

    // 3. 로컬 변수 (커서 위쪽 선언)
    items.extend(local_var_completions(src_text, pos));

    // 4. import 파일의 심볼
    if let Some(root_path) = uri_to_file_path(uri) {
        let base_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
        let mut seen: HashSet<PathBuf> = HashSet::new();
        if let Ok(canon) = fs::canonicalize(&root_path) {
            seen.insert(canon);
        } else {
            seen.insert(root_path.clone());
        }
        for raw_line in src_text.lines() {
            for line in expand_import_line(raw_line) {
                if let Some(rel) = parse_import_path(&line) {
                    let path = base_dir.join(&rel);
                    let resolved = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    if seen.insert(resolved.clone()) {
                        if let Ok(isrc) =
                            fs::read_to_string(&resolved).or_else(|_| fs::read_to_string(&path))
                        {
                            if let Ok(more) = catch_unwind(AssertUnwindSafe(|| {
                                extract_decl_completions_labeled(&isrc, &rel)
                            })) {
                                items.extend(more);
                            }
                        }
                    }
                }
            }
        }
    }

    // 중복 label 제거 (label 기준)
    let mut seen_labels: HashSet<String> = HashSet::new();
    items.retain(|i| seen_labels.insert(i.label.clone()));

    CompletionList {
        is_incomplete: false,
        items,
    }
}

// ────────────────────────────────────────────────────────────────────────────
//  `.` 뒤 컨텍스트 자동완성
// ────────────────────────────────────────────────────────────────────────────
fn dot_completions(text: &str, base: &str) -> Option<Vec<CompletionItem>> {
    let src = preprocess_source(text);
    let r = catch_unwind(AssertUnwindSafe(|| -> Option<Vec<CompletionItem>> {
        let tokens = Lexer::new(&src).tokenize();
        let ast = Parser::new(tokens).parse();

        // base가 enum 이름인지 확인
        for (item, _) in &ast.items {
            if let TopLevel::Enum { name, variants } = item {
                if name == base {
                    let items = variants
                        .iter()
                        .map(|v| {
                            let snip = if v.ty.is_some() {
                                format!("{}.{}(${{1:value}})", name, v.name)
                            } else {
                                format!("{}.{}", name, v.name)
                            };
                            let detail =
                                v.ty.as_ref()
                                    .map(|t| format!("({})", ty_str(t)))
                                    .unwrap_or_default();
                            CompletionItem {
                                label: v.name.clone(),
                                kind: Some(CompletionItemKind::ENUM_MEMBER),
                                detail: Some(format!("{}{}", v.name, detail)),
                                insert_text: Some(snip),
                                insert_text_format: Some(InsertTextFormat::SNIPPET),
                                ..Default::default()
                            }
                        })
                        .collect();
                    return Some(items);
                }
            }
        }

        // base가 변수 → 타입 추론 → struct field 완성
        let var_ty = super::hover::infer_var_type(text, base)?;
        for (item, _) in &ast.items {
            if let TopLevel::Struct { name, fields } = item {
                if *name == var_ty {
                    let items = fields
                        .iter()
                        .map(|f| CompletionItem {
                            label: f.name.clone(),
                            kind: Some(CompletionItemKind::FIELD),
                            detail: Some(ty_str(&f.ty)),
                            insert_text: Some(f.name.clone()),
                            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                            ..Default::default()
                        })
                        .collect();
                    return Some(items);
                }
            }
        }
        None
    }));
    r.ok().flatten()
}

// ────────────────────────────────────────────────────────────────────────────
//  로컬 변수 자동완성 (커서 위 선언 스캔)
// ────────────────────────────────────────────────────────────────────────────
fn local_var_completions(text: &str, pos: Position) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let scan_lines = (pos.line as usize).min(text.lines().count());

    for line in text.lines().take(scan_lines + 1) {
        let trimmed = line.trim();

        for (prefix, ty_label) in &[
            ("int ", "int"),
            ("float ", "float"),
            ("string ", "string"),
            ("bool ", "bool"),
            ("auto ", "auto"),
            ("i8 ", "i8"),
            ("i16 ", "i16"),
            ("i32 ", "i32"),
            ("i64 ", "i64"),
            ("u8 ", "u8"),
            ("u16 ", "u16"),
            ("u32 ", "u32"),
            ("u64 ", "u64"),
        ] {
            for strip_prefix in &[*prefix, &format!("Vault {}", prefix)] {
                if let Some(rest) = trimmed.strip_prefix(strip_prefix) {
                    let var: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !var.is_empty() && seen.insert(var.clone()) {
                        items.push(CompletionItem {
                            label: var.clone(),
                            kind: Some(CompletionItemKind::VARIABLE),
                            detail: Some(ty_label.to_string()),
                            insert_text: Some(var),
                            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        // for 루프 변수
        if let Some(rest) = trimmed.strip_prefix("for ") {
            let var: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !var.is_empty() && seen.insert(var.clone()) {
                items.push(CompletionItem {
                    label: var.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("int (loop var)".into()),
                    insert_text: Some(var),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    ..Default::default()
                });
            }
        }

        // 파라미터: `fn name(type param, ...)`  — 함수 파라미터를 스캔
        if trimmed.starts_with("fn ") {
            if let Some(params_start) = trimmed.find('(') {
                if let Some(params_end) = trimmed[params_start..].find(')') {
                    let params_str = &trimmed[params_start + 1..params_start + params_end];
                    for param in params_str.split(',') {
                        let parts: Vec<&str> = param.trim().splitn(2, ' ').collect();
                        if parts.len() == 2 {
                            let pname = parts[1].trim().to_string();
                            let pty = parts[0].trim().to_string();
                            if !pname.is_empty() && seen.insert(pname.clone()) {
                                items.push(CompletionItem {
                                    label: pname.clone(),
                                    kind: Some(CompletionItemKind::VARIABLE),
                                    detail: Some(pty),
                                    insert_text: Some(pname),
                                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    items
}

// ────────────────────────────────────────────────────────────────────────────
//  파일에서 함수/struct/enum/const 선언 추출
// ────────────────────────────────────────────────────────────────────────────
fn extract_decl_completions(src: &str) -> Vec<CompletionItem> {
    extract_decl_completions_labeled(src, "")
}

fn extract_decl_completions_labeled(src: &str, file_label: &str) -> Vec<CompletionItem> {
    let preprocessed = preprocess_source(src);
    let tokens = Lexer::new(&preprocessed).tokenize();
    let ast = Parser::new(tokens).parse();
    let mut items = Vec::new();
    let from = if file_label.is_empty() {
        String::new()
    } else {
        format!(" [from {}]", file_label)
    };

    for (item, _) in &ast.items {
        match item {
            TopLevel::Function {
                name,
                params,
                return_ty,
                ..
            } => {
                let ps: Vec<String> = params
                    .iter()
                    .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                    .collect();
                let ret = return_ty
                    .as_ref()
                    .map(|t| format!(" -> {}", ty_str(t)))
                    .unwrap_or_default();
                let snippet_params: String = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("${{{}:{}}}", i + 1, p.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(format!("fn({}){}{}", ps.join(", "), ret, from)),
                    insert_text: Some(format!("{}({})", name, snippet_params)),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
            TopLevel::Struct { name, fields } => {
                let field_snippets: String = fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| format!("{}: ${{{}:value}}", f.name, i + 1))
                    .collect::<Vec<_>>()
                    .join(", ");
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::STRUCT),
                    detail: Some(format!("struct{}", from)),
                    insert_text: Some(format!("{} {{ {} }}", name, field_snippets)),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
            TopLevel::Enum { name, .. } => {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::ENUM),
                    detail: Some(format!("enum{}", from)),
                    insert_text: Some(name.clone()),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    ..Default::default()
                });
            }
            TopLevel::ConstDecl { ty, name, .. } => {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::CONSTANT),
                    detail: Some(format!("const {}{}", ty_str(ty), from)),
                    insert_text: Some(name.clone()),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    ..Default::default()
                });
            }
            TopLevel::Module { name, .. } => {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some(format!("mod{}", from)),
                    insert_text: Some(format!("{}.", name)),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
    items
}

// ────────────────────────────────────────────────────────────────────────────
//  커서 앞 마지막 식별자 추출 (`base.` 에서 `base`)
// ────────────────────────────────────────────────────────────────────────────
fn extract_trailing_ident(s: &str) -> String {
    s.chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

// ────────────────────────────────────────────────────────────────────────────
//  키워드 스니펫
// ────────────────────────────────────────────────────────────────────────────
fn keyword_snippet_items() -> Vec<CompletionItem> {
    // (label, detail, snippet, kind)
    let snippets: &[(&str, &str, &str, CompletionItemKind)] = &[
        (
            "fn",
            "Function declaration",
            "fn ${1:name}(${2:int param}) -> ${3:int} {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "if",
            "Conditional branch",
            "if ${1:condition} {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "if else",
            "If-else branch",
            "if ${1:condition} {\n\t$0\n} else {\n\t\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "for",
            "Range-based loop",
            "for ${1:i} in ${2:0}..${3:n} {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "while",
            "Conditional loop",
            "while ${1:condition} {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "loop",
            "Infinite loop",
            "loop {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "match",
            "Pattern matching",
            "match ${1:expr} {\n\t${2:Pattern}.${3:Variant} => {\n\t\t$0\n\t}\n\t_ => {}\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "struct",
            "Struct declaration",
            "struct ${1:Name} {\n\t${2:int} ${3:field};\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "enum",
            "Enum declaration",
            "enum ${1:Name} {\n\t${2:Variant},\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "impl",
            "Trait implementation",
            "impl ${1:Trait} for ${2:Type} {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "trait",
            "Trait declaration",
            "trait ${1:Name} {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "mod",
            "Module declaration",
            "mod ${1:name} {\n\t$0\n}",
            CompletionItemKind::SNIPPET,
        ),
        (
            "print",
            "Print to stdout",
            "print($0);",
            CompletionItemKind::FUNCTION,
        ),
        (
            "assert",
            "Runtime assertion",
            "assert(${1:condition});",
            CompletionItemKind::FUNCTION,
        ),
        (
            "int",
            "64-bit signed integer",
            "int",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "float",
            "64-bit float",
            "float",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "string",
            "UTF-8 string",
            "string",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "bool",
            "Boolean",
            "bool",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "i8",
            "8-bit signed",
            "i8",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "i16",
            "16-bit signed",
            "i16",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "i32",
            "32-bit signed",
            "i32",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "i64",
            "64-bit signed",
            "i64",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "u8",
            "8-bit unsigned",
            "u8",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "u16",
            "16-bit unsigned",
            "u16",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "u32",
            "32-bit unsigned",
            "u32",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "u64",
            "64-bit unsigned",
            "u64",
            CompletionItemKind::TYPE_PARAMETER,
        ),
        (
            "auto",
            "Type inference",
            "auto",
            CompletionItemKind::KEYWORD,
        ),
        (
            "Vault",
            "Managed heap memory",
            "Vault",
            CompletionItemKind::KEYWORD,
        ),
        (
            "return",
            "Return value",
            "return $0;",
            CompletionItemKind::KEYWORD,
        ),
        ("break", "Exit loop", "break;", CompletionItemKind::KEYWORD),
        (
            "Kill",
            "Terminate anchor",
            "Kill \"${1:reason}\";",
            CompletionItemKind::KEYWORD,
        ),
        ("Exit", "Exit program", "Exit;", CompletionItemKind::KEYWORD),
        (
            "yield",
            "Yield from anchor",
            "yield $0;",
            CompletionItemKind::KEYWORD,
        ),
        ("true", "Boolean true", "true", CompletionItemKind::CONSTANT),
        (
            "false",
            "Boolean false",
            "false",
            CompletionItemKind::CONSTANT,
        ),
        (
            "catch",
            "Anchor error handler",
            "catch (string ${1:reason}) {\n\t$0\n\tbreak;\n}",
            CompletionItemKind::KEYWORD,
        ),
        (
            "emit",
            "Fire an event",
            "emit(\"${1:event}\", \"${2:payload}\");",
            CompletionItemKind::FUNCTION,
        ),
        (
            "import",
            "Import source file",
            "import \"${1:file.ky}\";",
            CompletionItemKind::KEYWORD,
        ),
        (
            "as",
            "Type cast",
            "as ${1:type}",
            CompletionItemKind::KEYWORD,
        ),
        (
            "const",
            "Named constant",
            "const ${1:int} ${2:NAME} = $0;",
            CompletionItemKind::KEYWORD,
        ),
    ];

    snippets
        .iter()
        .map(|&(label, detail, snippet, kind)| CompletionItem {
            label: label.into(),
            kind: Some(kind),
            detail: Some(detail.into()),
            insert_text: Some(snippet.into()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

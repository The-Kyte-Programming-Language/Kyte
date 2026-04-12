use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use lsp_server::{Connection, Message, Notification, Response};
use lsp_types::*;

use crate::analyzer::{Analyzer, CompileError, Severity};
use crate::ast::{TopLevel, Ty};
use crate::lexer::Lexer;
use crate::parser::Parser;

// ────────────────────────────────────────────────────────────
//  공개 진입점
// ────────────────────────────────────────────────────────────

pub fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    eprintln!("[kyte-lsp] starting …");

    let (conn, io) = Connection::stdio();

    let caps = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), "@".into()]),
            ..Default::default()
        }),
        ..Default::default()
    })?;

    let _init = conn.initialize(caps)?;
    eprintln!("[kyte-lsp] initialized");

    let mut docs: HashMap<Uri, String> = HashMap::new();

    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn.handle_shutdown(&req)? {
                    break;
                }
                dispatch_request(&conn, &req, &docs)?;
            }
            Message::Notification(not) => {
                dispatch_notification(&conn, &not, &mut docs)?;
            }
            Message::Response(_) => {}
        }
    }

    io.join()?;
    eprintln!("[kyte-lsp] shutdown");
    Ok(())
}

// ────────────────────────────────────────────────────────────
//  Notification 처리
// ────────────────────────────────────────────────────────────

fn dispatch_notification(
    conn: &Connection,
    not: &Notification,
    docs: &mut HashMap<Uri, String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match not.method.as_str() {
        "textDocument/didOpen" => {
            let p: DidOpenTextDocumentParams = serde_json::from_value(not.params.clone())?;
            let uri = p.text_document.uri.clone();
            let txt = p.text_document.text.clone();
            docs.insert(uri.clone(), txt.clone());
            send_diagnostics(conn, &uri, &txt)?;
        }
        "textDocument/didChange" => {
            let p: DidChangeTextDocumentParams = serde_json::from_value(not.params.clone())?;
            let uri = p.text_document.uri.clone();
            if let Some(c) = p.content_changes.into_iter().last() {
                docs.insert(uri.clone(), c.text.clone());
                send_diagnostics(conn, &uri, &c.text)?;
            }
        }
        "textDocument/didClose" => {
            let p: DidCloseTextDocumentParams = serde_json::from_value(not.params.clone())?;
            docs.remove(&p.text_document.uri);
            // 닫힐 때 진단 비우기
            let empty = PublishDiagnosticsParams {
                uri: p.text_document.uri,
                diagnostics: vec![],
                version: None,
            };
            conn.sender.send(Message::Notification(Notification {
                method: "textDocument/publishDiagnostics".into(),
                params: serde_json::to_value(empty)?,
            }))?;
        }
        _ => {}
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
//  Request 처리
// ────────────────────────────────────────────────────────────

fn dispatch_request(
    conn: &Connection,
    req: &lsp_server::Request,
    docs: &HashMap<Uri, String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match req.method.as_str() {
        "textDocument/hover" => {
            let p: HoverParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document_position_params.text_document.uri;
            let pos = p.text_document_position_params.position;
            let result = docs.get(uri).and_then(|t| compute_hover(t, pos));
            conn.sender.send(Message::Response(
                Response::new_ok(req.id.clone(), result),
            ))?;
        }
        "textDocument/completion" => {
            let p: CompletionParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document_position.text_document.uri;
            let list = compute_completions(docs.get(uri).map(|s: &String| s.as_str()));
            conn.sender.send(Message::Response(
                Response::new_ok(req.id.clone(), list),
            ))?;
        }
        _ => {
            conn.sender.send(Message::Response(
                Response::new_ok(req.id.clone(), serde_json::Value::Null),
            ))?;
        }
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
//  진단(Diagnostics)
// ────────────────────────────────────────────────────────────

fn send_diagnostics(
    conn: &Connection,
    uri: &Uri,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let diags = analyze_text(text);
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: diags,
        version: None,
    };
    conn.sender.send(Message::Notification(Notification {
        method: "textDocument/publishDiagnostics".into(),
        params: serde_json::to_value(params)?,
    }))?;
    Ok(())
}

fn is_import_line(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with("import") {
        return false;
    }
    let rest = t["import".len()..].trim_start();
    rest.starts_with('"') && rest.ends_with("\";")
}

fn preprocess_source(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        if is_import_line(line) {
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn analyze_text(text: &str) -> Vec<Diagnostic> {
    let src = preprocess_source(text);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut lex = Lexer::new(&src);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        let ast = par.parse();
        Analyzer::analyze(&ast, &src)
    }));

    match result {
        Ok(errs) => errs.iter().map(to_diagnostic).collect(),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "Syntax error".into());
            let (line, character) = parse_panic_position(&msg).unwrap_or((0, 0));
            vec![Diagnostic {
                range: Range {
                    start: Position { line, character },
                    end: Position { line, character: character.saturating_add(1) },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("kyte".into()),
                message: format!("Parse error: {}", msg),
                ..Default::default()
            }]
        }
    }
}

fn parse_panic_position(msg: &str) -> Option<(u32, u32)> {
    // panic 메시지 예시: "... at line 12:8"
    let marker = "line ";
    let start = msg.find(marker)? + marker.len();
    let rest = &msg[start..];

    let mut line_digits = String::new();
    let mut idx = 0usize;
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            line_digits.push(ch);
            idx += ch.len_utf8();
        } else {
            break;
        }
    }
    if line_digits.is_empty() {
        return None;
    }

    let rest = &rest[idx..];
    if !rest.starts_with(':') {
        return None;
    }
    let rest = &rest[1..];

    let mut col_digits = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            col_digits.push(ch);
        } else {
            break;
        }
    }
    if col_digits.is_empty() {
        return None;
    }

    let line_1: u32 = line_digits.parse().ok()?;
    let col_0: u32 = col_digits.parse().ok()?;
    Some((line_1.saturating_sub(1), col_0))
}

fn to_diagnostic(e: &CompileError) -> Diagnostic {
    let line = e.span.line.saturating_sub(1) as u32;
    let col = e.span.col as u32;
    let end_col = col + e.source_line.trim().len().max(1) as u32;

    Diagnostic {
        range: Range {
            start: Position { line, character: col },
            end: Position { line, character: end_col },
        },
        severity: Some(match e.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        code: Some(NumberOrString::String(e.code.to_string())),
        source: Some("kyte".into()),
        message: format!("{}\nhint: {}", e.message, e.hint),
        ..Default::default()
    }
}

// ────────────────────────────────────────────────────────────
//  호버(Hover)
// ────────────────────────────────────────────────────────────

fn compute_hover(text: &str, pos: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let col = pos.character as usize;
    if col >= chars.len() || !(chars[col].is_alphanumeric() || chars[col] == '_') {
        return None;
    }

    let mut lo = col;
    while lo > 0 && (chars[lo - 1].is_alphanumeric() || chars[lo - 1] == '_') {
        lo -= 1;
    }
    let mut hi = col;
    while hi < chars.len() && (chars[hi].is_alphanumeric() || chars[hi] == '_') {
        hi += 1;
    }
    let word: String = chars[lo..hi].iter().collect();

    let md = keyword_hover(&word)
        .or_else(|| symbol_hover(text, &word))?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(Range {
            start: Position { line: pos.line, character: lo as u32 },
            end: Position { line: pos.line, character: hi as u32 },
        }),
    })
}

fn keyword_hover(w: &str) -> Option<String> {
    let s = match w {
        "int" => "\
**int** — 64-bit signed integer type\n\n\
```kyte\n\
int x = 42;\n\
int y = x + 10;\n\
```",
        "float" => "\
**float** — 64-bit floating-point type\n\n\
```kyte\n\
float pi = 3.14;\n\
float r = pi * 2.0;\n\
```",
        "string" => "\
**string** — UTF-8 string type\n\n\
```kyte\n\
string name = \"world\";\n\
print(\"Hello, \" + name);\n\
```",
        "bool" => "\
**bool** — boolean type\n\n\
```kyte\n\
bool flag = true;\n\
if flag { print(1); }\n\
```",
        "fn" => "\
**fn** — declare a function\n\n\
```kyte\n\
fn add(int a, int b) -> int {\n\
    return a + b;\n\
}\n\
```",
        "struct" => "\
**struct** — user-defined data type\n\n\
```kyte\n\
struct User {\n\
    string name;\n\
    int age;\n\
}\n\
```",
        "Vault" => "\
**Vault** — managed-memory declaration (heap-allocated)\n\n\
```kyte\n\
Vault int buffer = 1024;\n\
// ... use buffer ...\n\
free(buffer);\n\
```",
        "yield" => "\
**yield** — transfer data out of an anchor\n\n\
```kyte\n\
@producer() {\n\
    yield 42;\n\
}\n\
```",
        "print" => "\
**print(...)** — print values to stdout\n\n\
```kyte\n\
print(42);\n\
print(\"hello\");\n\
print(x + y);\n\
```",
        "Kill" => "\
**Kill** — terminate the current anchor with recovery\n\n\
```kyte\n\
@handler() {\n\
    Kill \"error occurred\";\n\
}\n\
```",
        "Exit" => "\
**Exit** — exit the entire program\n\n\
```kyte\n\
if error {\n\
    Exit;\n\
}\n\
```",
        "return" => "\
**return** — return a value from a function\n\n\
```kyte\n\
fn double(int n) -> int {\n\
    return n * 2;\n\
}\n\
```",
        "if" => "\
**if** — conditional branch\n\n\
```kyte\n\
if x > 10 {\n\
    print(\"big\");\n\
} else {\n\
    print(\"small\");\n\
}\n\
```",
        "else" => "\
**else** — alternative branch\n\n\
```kyte\n\
if x > 0 {\n\
    print(\"positive\");\n\
} else {\n\
    print(\"non-positive\");\n\
}\n\
```",
        "loop" => "\
**loop** — infinite loop (use `break` to exit)\n\n\
```kyte\n\
int i = 0;\n\
loop {\n\
    if i >= 10 { break; }\n\
    i += 1;\n\
}\n\
```",
        "while" => "\
**while** — conditional loop\n\n\
```kyte\n\
int i = 0;\n\
while i < 10 {\n\
    print(i);\n\
    i += 1;\n\
}\n\
```",
        "for" => "\
**for** — range-based loop\n\n\
```kyte\n\
for i in 0..5 {\n\
    print(i);  // 0, 1, 2, 3, 4\n\
}\n\
```",
        "break" => "\
**break** — exit the innermost loop\n\n\
```kyte\n\
loop {\n\
    if done { break; }\n\
}\n\
```",
        "true" => "\
**true** — boolean literal\n\n\
```kyte\n\
bool active = true;\n\
```",
        "false" => "\
**false** — boolean literal\n\n\
```kyte\n\
bool done = false;\n\
```",
        "as" => "\
**as** — type casting\n\n\
```kyte\n\
int x = 42;\n\
float y = x as float;\n\
```",
        "import" => "\
    **import** — include another Kyte source file\n\n\
    ```kyte\n\
    import \"util.ky\";\n\
    @main(main) {\n\
        print(add(1, 2));\n\
    }\n\
    ```",
        "free" => "\
**free(name)** — release Vault memory\n\n\
```kyte\n\
Vault int buf = 512;\n\
free(buf);\n\
```",
        _ => return None,
    };
    Some(s.into())
}

fn symbol_hover(text: &str, word: &str) -> Option<String> {
    let src = preprocess_source(text);
    let r = catch_unwind(AssertUnwindSafe(|| -> Option<String> {
        let mut lex = Lexer::new(&src);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        let ast = par.parse();

        for (item, span) in &ast.items {
            match item {
                TopLevel::Function { name, params, return_ty, .. } if name == word => {
                    let ps: Vec<String> = params
                        .iter()
                        .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                        .collect();
                    let ret = return_ty
                        .as_ref()
                        .map(|t| format!(" -> {}", ty_str(t)))
                        .unwrap_or_default();

                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!("```kyte\nfn {}({}){}\n```", name, ps.join(", "), ret);
                    let hover_md = format_with_doc(&sig, &doc);
                    return Some(hover_md);
                }
                TopLevel::Anchor { name, kind, .. } if name == word => {
                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!("```kyte\n@{}({:?})\n```", name, kind);
                    return Some(format_with_doc(&sig, &doc));
                }
                // 중첩 앵커 탐색
                TopLevel::Anchor { children, .. } => {
                    if let Some(result) = search_children(&src, children, word) {
                        return Some(result);
                    }
                }
                _ => {}
            }
        }
        None
    }));
    r.ok().flatten()
}

fn search_children(src: &str, children: &[(TopLevel, crate::ast::Span)], word: &str) -> Option<String> {
    for (item, span) in children {
        if let TopLevel::Anchor { name, kind, children: nested, .. } = item {
            if name == word {
                let doc = extract_doc_comment(src, span.line);
                let sig = format!("```kyte\n@{}({:?})\n```", name, kind);
                return Some(format_with_doc(&sig, &doc));
            }
            if let Some(result) = search_children(src, nested, word) {
                return Some(result);
            }
        }
    }
    None
}

fn format_with_doc(sig: &str, doc: &str) -> String {
    if doc.is_empty() {
        sig.to_string()
    } else {
        // doc 안의 ```kyte 를 ``` 로 통일 (VS Code 호버에서 인식 보장)
        let doc_normalized = doc.replace("```kyte", "```");
        format!("{}\n\n---\n\n{}", sig, doc_normalized)
    }
}

/// `fn` 선언(1-indexed `fn_line`) 바로 위에 있는 연속된 `///` 주석을 추출한다.
/// 들여쓰기(4칸+)된 줄은 자동으로 코드 블록으로 감싼다.
fn extract_doc_comment(src: &str, fn_line: usize) -> String {
    let lines: Vec<&str> = src.lines().collect();
    if fn_line == 0 || fn_line > lines.len() {
        return String::new();
    }
    // fn_line은 1-indexed → 배열에선 fn_line-1.  그 바로 위부터 위로 스캔
    let mut doc_lines: Vec<&str> = Vec::new();
    let mut idx = fn_line as isize - 2; // 바로 윗줄(0-indexed)
    while idx >= 0 {
        let trimmed = lines[idx as usize].trim();
        if let Some(rest) = trimmed.strip_prefix("///") {
            doc_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
            idx -= 1;
        } else {
            break;
        }
    }
    doc_lines.reverse();

    // 들여쓰기(4칸+)된 줄을 자동으로 ```kyte 코드 블록으로 감싸기
    let mut result = String::new();
    let mut in_code = false;
    let mut in_user_fence = false;

    for line in &doc_lines {
        // 사용자가 직접 ``` 를 쓴 경우 그대로 통과
        if line.trim().starts_with("```") {
            in_user_fence = !in_user_fence;
            result.push_str(line);
            result.push('\n');
            continue;
        }
        if in_user_fence {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        let is_code_line = line.starts_with("    ") || line.starts_with('\t');
        if is_code_line && !in_code {
            result.push_str("```\n");
            in_code = true;
        } else if !is_code_line && in_code {
            result.push_str("```\n");
            in_code = false;
        }

        if in_code {
            // 들여쓰기 4칸 제거
            let stripped = if line.starts_with("    ") {
                &line[4..]
            } else if line.starts_with('\t') {
                &line[1..]
            } else {
                line
            };
            result.push_str(stripped);
            result.push('\n');
        } else {
            result.push_str(line);
            // Markdown에서 줄바꿈 유지 + 한 줄 여백으로 가독성 개선
            result.push_str("  \n\n");
        }
    }
    if in_code {
        result.push_str("```\n");
    }

    result.trim_end().to_string()
}

fn ty_str(ty: &Ty) -> String {
    match ty {
        Ty::Int => "int".to_string(),
        Ty::Float => "float".to_string(),
        Ty::String => "string".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::I8  => "i8".to_string(),
        Ty::I16 => "i16".to_string(),
        Ty::I32 => "i32".to_string(),
        Ty::I64 => "i64".to_string(),
        Ty::U8  => "u8".to_string(),
        Ty::U16 => "u16".to_string(),
        Ty::U32 => "u32".to_string(),
        Ty::U64 => "u64".to_string(),
        Ty::Array(inner) => format!("{}[]", ty_str(inner)),
        Ty::Struct(name) => name.clone(),
    }
}

// ────────────────────────────────────────────────────────────
//  자동완성(Completion)
// ────────────────────────────────────────────────────────────

fn compute_completions(text: Option<&str>) -> CompletionList {
    let mut items: Vec<CompletionItem> = KEYWORDS
        .iter()
        .map(|&(label, kind, detail)| CompletionItem {
            label: label.into(),
            kind: Some(kind),
            detail: Some(detail.into()),
            ..Default::default()
        })
        .collect();

    // 문서에서 함수 이름도 추가
    if let Some(src) = text {
        let src = src.to_string();
        if let Ok(fns) = catch_unwind(AssertUnwindSafe(|| extract_fn_names(&src))) {
            for (name, sig) in fns {
                items.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(sig),
                    ..Default::default()
                });
            }
        }
    }

    CompletionList { is_incomplete: false, items }
}

fn extract_fn_names(src: &str) -> Vec<(String, String)> {
    let src = preprocess_source(src);
    let mut lex = Lexer::new(&src);
    let tokens = lex.tokenize();
    let mut par = Parser::new(tokens);
    let ast = par.parse();
    let mut out = Vec::new();
    for (item, _) in &ast.items {
        if let TopLevel::Function { name, params, return_ty, .. } = item {
            let ps: Vec<String> = params.iter().map(|p| format!("{} {}", ty_str(&p.ty), p.name)).collect();
            let ret = return_ty.as_ref().map(|t| format!(" -> {}", ty_str(t))).unwrap_or_default();
            out.push((name.clone(), format!("fn({}){}", ps.join(", "), ret)));
        }
    }
    out
}

const KEYWORDS: &[(&str, CompletionItemKind, &str)] = &[
    ("fn",     CompletionItemKind::KEYWORD,        "Function declaration"),
    ("struct", CompletionItemKind::KEYWORD,        "Struct type declaration"),
    ("int",    CompletionItemKind::TYPE_PARAMETER,  "64-bit signed integer (alias for i64)"),
    ("float",  CompletionItemKind::TYPE_PARAMETER,  "64-bit float"),
    ("string", CompletionItemKind::TYPE_PARAMETER,  "String type"),
    ("bool",   CompletionItemKind::TYPE_PARAMETER,  "Boolean type"),
    ("i8",     CompletionItemKind::TYPE_PARAMETER,  "8-bit signed integer"),
    ("i16",    CompletionItemKind::TYPE_PARAMETER,  "16-bit signed integer"),
    ("i32",    CompletionItemKind::TYPE_PARAMETER,  "32-bit signed integer"),
    ("i64",    CompletionItemKind::TYPE_PARAMETER,  "64-bit signed integer"),
    ("u8",     CompletionItemKind::TYPE_PARAMETER,  "8-bit unsigned integer"),
    ("u16",    CompletionItemKind::TYPE_PARAMETER,  "16-bit unsigned integer"),
    ("u32",    CompletionItemKind::TYPE_PARAMETER,  "32-bit unsigned integer"),
    ("u64",    CompletionItemKind::TYPE_PARAMETER,  "64-bit unsigned integer"),
    ("if",     CompletionItemKind::KEYWORD,        "Conditional"),
    ("else",   CompletionItemKind::KEYWORD,        "Alternative branch"),
    ("for",    CompletionItemKind::KEYWORD,        "Range loop"),
    ("loop",   CompletionItemKind::KEYWORD,        "Infinite loop"),
    ("while",  CompletionItemKind::KEYWORD,        "Conditional loop"),
    ("break",  CompletionItemKind::KEYWORD,        "Exit loop"),
    ("return", CompletionItemKind::KEYWORD,        "Return value"),
    ("yield",  CompletionItemKind::KEYWORD,        "Transfer data out of anchor"),
    ("Vault",  CompletionItemKind::KEYWORD,        "Managed heap memory"),
    ("free",   CompletionItemKind::FUNCTION,       "Release Vault memory"),
    ("print",  CompletionItemKind::FUNCTION,       "Print values to stdout"),
    ("as",     CompletionItemKind::KEYWORD,        "Type cast: expr as type"),
    ("import", CompletionItemKind::KEYWORD,        "Import another .ky source file"),
    ("Kill",   CompletionItemKind::KEYWORD,        "Terminate anchor with recovery"),
    ("Exit",   CompletionItemKind::KEYWORD,        "Exit program"),
    ("true",   CompletionItemKind::CONSTANT,       "Boolean true"),
    ("false",  CompletionItemKind::CONSTANT,       "Boolean false"),
];

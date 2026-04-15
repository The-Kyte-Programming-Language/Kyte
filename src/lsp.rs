use std::collections::{HashMap, HashSet};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

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
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), "@".into()]),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            ..Default::default()
        }),
        ..Default::default()
    })?;

    let _init = conn.initialize(caps)?;
    eprintln!("[kyte-lsp] initialized");

    #[allow(clippy::mutable_key_type)]
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

#[allow(clippy::mutable_key_type)]
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

#[allow(clippy::mutable_key_type)]
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
            conn.sender
                .send(Message::Response(Response::new_ok(req.id.clone(), result)))?;
        }
        "textDocument/completion" => {
            let p: CompletionParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document_position.text_document.uri;
            let list = compute_completions(docs.get(uri).map(|s: &String| s.as_str()));
            conn.sender
                .send(Message::Response(Response::new_ok(req.id.clone(), list)))?;
        }
        "textDocument/definition" => {
            let p: GotoDefinitionParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document_position_params.text_document.uri;
            let pos = p.text_document_position_params.position;
            let result = docs.get(uri).and_then(|t| compute_definition(t, pos, uri));
            conn.sender
                .send(Message::Response(Response::new_ok(req.id.clone(), result)))?;
        }
        "textDocument/references" => {
            let p: ReferenceParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document_position.text_document.uri;
            let pos = p.text_document_position.position;
            let result = docs.get(uri).map(|t| compute_references(t, pos, uri));
            conn.sender
                .send(Message::Response(Response::new_ok(req.id.clone(), result)))?;
        }
        "textDocument/rename" => {
            let p: RenameParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document_position.text_document.uri;
            let pos = p.text_document_position.position;
            let new_name = &p.new_name;
            let result = docs
                .get(uri)
                .and_then(|t| compute_rename(t, pos, uri, new_name));
            conn.sender
                .send(Message::Response(Response::new_ok(req.id.clone(), result)))?;
        }
        "textDocument/documentSymbol" => {
            let p: DocumentSymbolParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document.uri;
            let result = docs.get(uri).map(|t| compute_document_symbols(t, uri));
            conn.sender
                .send(Message::Response(Response::new_ok(req.id.clone(), result)))?;
        }
        "textDocument/signatureHelp" => {
            let p: SignatureHelpParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document_position_params.text_document.uri;
            let pos = p.text_document_position_params.position;
            let result = docs.get(uri).and_then(|t| compute_signature_help(t, pos));
            conn.sender
                .send(Message::Response(Response::new_ok(req.id.clone(), result)))?;
        }
        _ => {
            conn.sender.send(Message::Response(Response::new_ok(
                req.id.clone(),
                serde_json::Value::Null,
            )))?;
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
    let diags = analyze_text(uri, text);
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
    parse_import_path(line).is_some()
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

fn append_non_import_lines(src: &str, out: &mut String) {
    for line in src.lines() {
        if is_import_line(line) {
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
}

fn visit_import_file(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut String) {
    // canonicalize 실패해도(Windows에서 흔함) 원본 경로로 폴백
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    if !seen.insert(resolved.clone()) {
        return;
    }

    let text = match fs::read_to_string(&resolved).or_else(|_| fs::read_to_string(path)) {
        Ok(t) => t,
        Err(_) => return,
    };

    let base_dir = resolved.parent()
        .or_else(|| path.parent())
        .unwrap_or_else(|| Path::new("."));

    for line in text.lines() {
        if let Some(rel) = parse_import_path(line) {
            visit_import_file(&base_dir.join(rel), seen, out);
        }
    }

    out.push_str(&format!(
        "\n// ---- import file: {} ----\n",
        resolved.display()
    ));
    append_non_import_lines(&text, out);
}

fn parse_import_path(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with("import") {
        return None;
    }
    let rest = t["import".len()..].trim_start();
    let raw_path = rest.strip_suffix(';')?.trim();
    if raw_path.is_empty() {
        return None;
    }
    if raw_path.starts_with('"') && raw_path.ends_with('"') && raw_path.len() >= 2 {
        return Some(raw_path[1..raw_path.len() - 1].to_string());
    }
    Some(raw_path.to_string())
}

fn uri_to_file_path(uri: &Uri) -> Option<PathBuf> {
    let raw = uri.to_string();
    if !raw.starts_with("file://") {
        return None;
    }
    let mut path = raw.trim_start_matches("file://").to_string();
    // URL 디코딩
    path = path.replace("%20", " ");
    path = path.replace("%3A", ":").replace("%3a", ":");
    // Windows file URI: file:///C:/... -> /C:/... -> C:/...
    // 슬래시로 시작하고 두 번째 문자 뒤에 콜론이 있으면 Windows 드라이브 경로
    if path.starts_with('/') {
        let rest = &path[1..];
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
            path = rest.to_string();
        }
    }
    // 백슬래시 정규화 (혹시 섞여있을 경우)
    path = path.replace('/', std::path::MAIN_SEPARATOR_STR);
    Some(PathBuf::from(path))
}

fn preprocess_source_with_imports(uri: &Uri, text: &str) -> (String, usize, usize) {
    let own_line_count = text.lines().count();
    let mut merged = String::new();

    let root_path = match uri_to_file_path(uri) {
        Some(p) => p,
        None => return (preprocess_source(text), 0, own_line_count),
    };
    let base_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
    let mut seen = HashSet::new();
    if let Ok(canon) = fs::canonicalize(&root_path) {
        seen.insert(canon);
    }

    // import 파일을 먼저 — 현재 파일보다 앞에 붙여야
    // analyzer가 함수 선언을 사용 전에 볼 수 있음
    for line in text.lines() {
        if let Some(rel) = parse_import_path(line) {
            visit_import_file(&base_dir.join(rel), &mut seen, &mut merged);
        }
    }

    // import 파일들이 차지하는 라인 수
    let import_line_offset = merged.lines().count();

    // 현재 파일은 나중에
    append_non_import_lines(text, &mut merged);

    (merged, import_line_offset, own_line_count)
}

fn analyze_text(uri: &Uri, text: &str) -> Vec<Diagnostic> {
    let (src, import_line_offset, own_line_count) = preprocess_source_with_imports(uri, text);
    let has_main_anchor = text.contains("@") && text.contains("(main)");
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut lex = Lexer::new(&src);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        let ast = par.parse();
        Analyzer::analyze(&ast, &src)
    }));

    match result {
        Ok(errs) => errs
            .into_iter()
            .filter(|e| {
                // import 파일 라인(앞부분)의 진단은 제외, 현재 파일 라인만 표시
                if e.span.line <= import_line_offset {
                    return false;
                }
                if e.span.line > import_line_offset + own_line_count.max(1) {
                    return false;
                }
                // 라이브러리/helper 파일에서는 main 앵커 강제 진단 숨김
                if !has_main_anchor && matches!(e.code, "E018" | "E019" | "E022") {
                    return false;
                }
                true
            })
            .map(|e| {
                // 라인 번호에서 import offset을 빼서 현재 파일 기준으로 변환
                let mut adjusted = e.clone();
                adjusted.span.line = adjusted.span.line.saturating_sub(import_line_offset);
                to_diagnostic(&adjusted)
            })
            .collect(),
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
                    end: Position {
                        line,
                        character: character.saturating_add(1),
                    },
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
            start: Position {
                line,
                character: col,
            },
            end: Position {
                line,
                character: end_col,
            },
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

    let md = keyword_hover(&word).or_else(|| symbol_hover(text, &word))?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(Range {
            start: Position {
                line: pos.line,
                character: lo as u32,
            },
            end: Position {
                line: pos.line,
                character: hi as u32,
            },
        }),
    })
}

fn keyword_hover(w: &str) -> Option<String> {
    let s = match w {
        "enum" => {
            "\
**enum** — enum type declaration\n\n\
```kyte\n\
enum Color {\n\
    Red,\n\
    Green,\n\
    Blue,\n\
}\n\
```"
        }
        "match" => {
            "\
**match** — pattern matching\n\n\
```kyte\n\
match color {\n\
    Color.Red => { print(\"red\"); }\n\
    Color.Green => { print(\"green\"); }\n\
    _ => { print(\"other\"); }\n\
}\n\
```"
        }
        "int" => {
            "\
**int** — 64-bit signed integer type\n\n\
```kyte\n\
int x = 42;\n\
int y = x + 10;\n\
```"
        }
        "float" => {
            "\
**float** — 64-bit floating-point type\n\n\
```kyte\n\
float pi = 3.14;\n\
float r = pi * 2.0;\n\
```"
        }
        "string" => {
            "\
**string** — UTF-8 string type\n\n\
```kyte\n\
string name = \"world\";\n\
print(\"Hello, \" + name);\n\
```"
        }
        "bool" => {
            "\
**bool** — boolean type\n\n\
```kyte\n\
bool flag = true;\n\
if flag { print(1); }\n\
```"
        }
        "fn" => {
            "\
**fn** — declare a function\n\n\
```kyte\n\
fn add(int a, int b) -> int {\n\
    return a + b;\n\
}\n\
```"
        }
        "struct" => {
            "\
**struct** — user-defined data type\n\n\
```kyte\n\
struct User {\n\
    string name;\n\
    int age;\n\
}\n\
```"
        }
        "Vault" => {
            "\
**Vault** — managed-memory declaration (heap-allocated)\n\n\
Vault variables are automatically freed at scope exit.\n\n\
```kyte\n\
Vault int buffer = 1024;\n\
// ... use buffer ...\n\
// automatically freed when scope ends\n\
```"
        }
        "yield" => {
            "\
**yield** — transfer data out of an anchor\n\n\
```kyte\n\
@producer() {\n\
    yield 42;\n\
}\n\
```"
        }
        "print" => {
            "\
**print(...)** — print values to stdout\n\n\
```kyte\n\
print(42);\n\
print(\"hello\");\n\
print(x + y);\n\
```"
        }
        "Kill" => {
            "\
**Kill** — terminate the current anchor with recovery\n\n\
```kyte\n\
@handler() {\n\
    Kill \"error occurred\";\n\
}\n\
```"
        }
        "Exit" => {
            "\
**Exit** — exit the entire program\n\n\
```kyte\n\
if error {\n\
    Exit;\n\
}\n\
```"
        }
        "return" => {
            "\
**return** — return a value from a function\n\n\
```kyte\n\
fn double(int n) -> int {\n\
    return n * 2;\n\
}\n\
```"
        }
        "if" => {
            "\
**if** — conditional branch\n\n\
```kyte\n\
if x > 10 {\n\
    print(\"big\");\n\
} else {\n\
    print(\"small\");\n\
}\n\
```"
        }
        "else" => {
            "\
**else** — alternative branch\n\n\
```kyte\n\
if x > 0 {\n\
    print(\"positive\");\n\
} else {\n\
    print(\"non-positive\");\n\
}\n\
```"
        }
        "loop" => {
            "\
**loop** — infinite loop (use `break` to exit)\n\n\
```kyte\n\
int i = 0;\n\
loop {\n\
    if i >= 10 { break; }\n\
    i += 1;\n\
}\n\
```"
        }
        "while" => {
            "\
**while** — conditional loop\n\n\
```kyte\n\
int i = 0;\n\
while i < 10 {\n\
    print(i);\n\
    i += 1;\n\
}\n\
```"
        }
        "for" => {
            "\
**for** — range-based loop\n\n\
```kyte\n\
for i in 0..5 {\n\
    print(i);  // 0, 1, 2, 3, 4\n\
}\n\
```"
        }
        "break" => {
            "\
**break** — exit the innermost loop\n\n\
```kyte\n\
loop {\n\
    if done { break; }\n\
}\n\
```"
        }
        "true" => {
            "\
**true** — boolean literal\n\n\
```kyte\n\
bool active = true;\n\
```"
        }
        "false" => {
            "\
**false** — boolean literal\n\n\
```kyte\n\
bool done = false;\n\
```"
        }
        "as" => {
            "\
**as** — type casting\n\n\
```kyte\n\
int x = 42;\n\
float y = x as float;\n\
```"
        }
        "import" => {
            "\
    **import** — include another Kyte source file\n\n\
    ```kyte\n\
    import \"util.ky\";\n\
    @main(main) {\n\
        print(add(1, 2));\n\
    }\n\
    ```"
        }
        "free" => {
            "\
~~**free(name)**~~ — **deprecated** (E033)\n\n\
Manual `free()` is no longer allowed.\n\
Vault variables are automatically freed at scope exit.\n\n\
```kyte\n\
Vault int buf = 512;\n\
// buf is automatically freed when scope ends\n\
```"
        }
        "auto" => {
            "\
**auto** — infer the type from the initializer\n\n\
```kyte\n\
auto x = 42;       // int\n\
auto name = \"hi\"; // string\n\
auto flag = true;  // bool\n\
```"
        }
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
                TopLevel::Function {
                    name,
                    params,
                    return_ty,
                    ..
                } if name == word => {
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

fn search_children(
    src: &str,
    children: &[(TopLevel, crate::ast::Span)],
    word: &str,
) -> Option<String> {
    for (item, span) in children {
        if let TopLevel::Anchor {
            name,
            kind,
            children: nested,
            ..
        } = item
        {
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

// ────────────────────────────────────────────────────────────
//  Go-to-Definition
// ────────────────────────────────────────────────────────────

fn compute_definition(text: &str, pos: Position, uri: &Uri) -> Option<GotoDefinitionResponse> {
    let word = word_at(text, pos)?;
    let src = preprocess_source(text);
    let r = catch_unwind(AssertUnwindSafe(|| -> Option<Location> {
        let mut lex = Lexer::new(&src);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        let ast = par.parse();

        for (item, span) in &ast.items {
            let found = match item {
                TopLevel::Function { name, .. } if *name == word => true,
                TopLevel::Struct { name, .. } if *name == word => true,
                TopLevel::Enum { name, .. } if *name == word => true,
                TopLevel::Anchor { name, .. } if *name == word => true,
                _ => false,
            };
            if found {
                let line = span.line.saturating_sub(1) as u32;
                return Some(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: Position { line, character: 0 },
                        end: Position {
                            line,
                            character: word.len() as u32 + 10,
                        },
                    },
                });
            }
            // search nested anchors
            if let TopLevel::Anchor { children, .. } = item {
                if let Some(loc) = find_def_in_children(children, &word, uri) {
                    return Some(loc);
                }
            }
        }
        None
    }));
    r.ok().flatten().map(GotoDefinitionResponse::Scalar)
}

fn find_def_in_children(
    children: &[(TopLevel, crate::ast::Span)],
    word: &str,
    uri: &Uri,
) -> Option<Location> {
    for (item, span) in children {
        let found = match item {
            TopLevel::Function { name, .. } if name == word => true,
            TopLevel::Struct { name, .. } if name == word => true,
            TopLevel::Enum { name, .. } if name == word => true,
            TopLevel::Anchor { name, .. } if name == word => true,
            _ => false,
        };
        if found {
            let line = span.line.saturating_sub(1) as u32;
            return Some(Location {
                uri: uri.clone(),
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position {
                        line,
                        character: word.len() as u32 + 10,
                    },
                },
            });
        }
        if let TopLevel::Anchor {
            children: nested, ..
        } = item
        {
            if let Some(loc) = find_def_in_children(nested, word, uri) {
                return Some(loc);
            }
        }
    }
    None
}

// ────────────────────────────────────────────────────────────
//  Find References
// ────────────────────────────────────────────────────────────

fn compute_references(text: &str, pos: Position, uri: &Uri) -> Vec<Location> {
    let word = match word_at(text, pos) {
        Some(w) => w,
        None => return vec![],
    };
    // Simple text-based search: find all occurrences of the word as a whole word
    let mut refs = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let mut col = 0usize;
        let hay = line;
        while let Some(offset) = hay[col..].find(&word) {
            let abs = col + offset;
            // Ensure it's a whole word
            let before_ok = abs == 0 || {
                let c = hay.as_bytes()[abs - 1];
                !(c.is_ascii_alphanumeric() || c == b'_')
            };
            let after_pos = abs + word.len();
            let after_ok = after_pos >= hay.len() || {
                let c = hay.as_bytes()[after_pos];
                !(c.is_ascii_alphanumeric() || c == b'_')
            };
            if before_ok && after_ok {
                refs.push(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: Position {
                            line: line_idx as u32,
                            character: abs as u32,
                        },
                        end: Position {
                            line: line_idx as u32,
                            character: after_pos as u32,
                        },
                    },
                });
            }
            col = abs + word.len().max(1);
        }
    }
    refs
}

/// Extract the word under the cursor
fn word_at(text: &str, pos: Position) -> Option<String> {
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
    Some(chars[lo..hi].iter().collect())
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
            let stripped = if let Some(s) = line.strip_prefix("    ") {
                s
            } else if let Some(s) = line.strip_prefix('\t') {
                s
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
        Ty::I8 => "i8".to_string(),
        Ty::I16 => "i16".to_string(),
        Ty::I32 => "i32".to_string(),
        Ty::I64 => "i64".to_string(),
        Ty::U8 => "u8".to_string(),
        Ty::U16 => "u16".to_string(),
        Ty::U32 => "u32".to_string(),
        Ty::U64 => "u64".to_string(),
        Ty::Array(inner) => format!("{}[]", ty_str(inner)),
        Ty::Struct(name) => name.clone(),
        Ty::Auto => "auto".to_string(),
        Ty::Enum(name) => name.clone(),
        Ty::TypeParam(name) => name.clone(),
        Ty::Fn(params, ret) => {
            let ps: Vec<String> = params.iter().map(ty_str).collect();
            let ret_s = ret
                .as_deref()
                .map(ty_str)
                .unwrap_or_else(|| "void".to_string());
            format!("fn({}) -> {}", ps.join(", "), ret_s)
        }
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

// ────────────────────────────────────────────────────────────
//  Rename (textDocument/rename)
// ────────────────────────────────────────────────────────────

fn compute_rename(text: &str, pos: Position, uri: &Uri, new_name: &str) -> Option<WorkspaceEdit> {
    // 커서 위치에 단어 있는지 확인
    word_at(text, pos)?;
    // 모든 참조를 찾아 new_name으로 교체
    let refs = compute_references(text, pos, uri);
    if refs.is_empty() {
        return None;
    }
    let edits: Vec<TextEdit> = refs
        .into_iter()
        .map(|loc| TextEdit {
            range: loc.range,
            new_text: new_name.to_string(),
        })
        .collect();
    #[allow(clippy::mutable_key_type)]
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

// ────────────────────────────────────────────────────────────
//  Document Symbols (textDocument/documentSymbol)
// ────────────────────────────────────────────────────────────

fn compute_document_symbols(text: &str, uri: &Uri) -> Vec<SymbolInformation> {
    let src = preprocess_source(text);
    let mut lex = Lexer::new(&src);
    let tokens = lex.tokenize();
    let mut par = Parser::new(tokens);
    let ast = par.parse();
    let mut syms: Vec<SymbolInformation> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();

    let mk_range = |line_1indexed: usize| -> Range {
        let ln = (line_1indexed.saturating_sub(1)) as u32;
        Range {
            start: Position {
                line: ln,
                character: 0,
            },
            end: Position {
                line: ln,
                character: lines.get(ln as usize).map(|l| l.len() as u32).unwrap_or(0),
            },
        }
    };

    for (item, span) in &ast.items {
        match item {
            TopLevel::Function { name, .. } => {
                #[allow(deprecated)]
                syms.push(SymbolInformation {
                    name: name.clone(),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: mk_range(span.line),
                    },
                    container_name: None,
                });
            }
            TopLevel::Struct { name, .. } => {
                #[allow(deprecated)]
                syms.push(SymbolInformation {
                    name: name.clone(),
                    kind: SymbolKind::STRUCT,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: mk_range(span.line),
                    },
                    container_name: None,
                });
            }
            TopLevel::Enum { name, .. } => {
                #[allow(deprecated)]
                syms.push(SymbolInformation {
                    name: name.clone(),
                    kind: SymbolKind::ENUM,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: mk_range(span.line),
                    },
                    container_name: None,
                });
            }
            TopLevel::Trait { name, .. } => {
                #[allow(deprecated)]
                syms.push(SymbolInformation {
                    name: name.clone(),
                    kind: SymbolKind::INTERFACE,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: mk_range(span.line),
                    },
                    container_name: None,
                });
            }
            TopLevel::Module { name, .. } => {
                #[allow(deprecated)]
                syms.push(SymbolInformation {
                    name: name.clone(),
                    kind: SymbolKind::MODULE,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: mk_range(span.line),
                    },
                    container_name: None,
                });
            }
            _ => {}
        }
    }
    syms
}

// ────────────────────────────────────────────────────────────
//  Signature Help (textDocument/signatureHelp)
// ────────────────────────────────────────────────────────────

fn compute_signature_help(text: &str, pos: Position) -> Option<SignatureHelp> {
    // 현재 줄에서 커서 왼쪽으로 미완성 호출 찾기: foo(
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(pos.line as usize)?;
    let col = (pos.character as usize).min(line.len());
    let before = &line[..col];

    // 가장 마지막 '(' 찾기
    let paren_pos = before.rfind('(')?;
    let fn_part = before[..paren_pos].trim_end();
    // 함수 이름 추출
    let fn_name: String = fn_part
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if fn_name.is_empty() {
        return None;
    }

    // 현재 몇 번째 인수인지 쉼표 카운트
    let active_param = before[paren_pos + 1..]
        .chars()
        .filter(|&c| c == ',')
        .count() as u32;

    // AST에서 함수 정의 찾기
    let src = preprocess_source(text);
    let tokens = Lexer::new(&src).tokenize();
    let mut par = Parser::new(tokens);
    let ast = par.parse();
    for (item, _) in &ast.items {
        if let TopLevel::Function {
            name,
            params,
            return_ty,
            ..
        } = item
        {
            if name != &fn_name {
                continue;
            }
            let ps: Vec<String> = params
                .iter()
                .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                .collect();
            let ret = return_ty
                .as_ref()
                .map(|t| format!(" -> {}", ty_str(t)))
                .unwrap_or_default();
            let label = format!("fn {}({}){}", name, ps.join(", "), ret);
            let param_infos: Vec<ParameterInformation> = params
                .iter()
                .map(|p| ParameterInformation {
                    label: ParameterLabel::Simple(format!("{} {}", ty_str(&p.ty), p.name)),
                    documentation: None,
                })
                .collect();
            return Some(SignatureHelp {
                signatures: vec![SignatureInformation {
                    label,
                    documentation: None,
                    parameters: Some(param_infos),
                    active_parameter: Some(active_param),
                }],
                active_signature: Some(0),
                active_parameter: Some(active_param),
            });
        }
    }
    None
}
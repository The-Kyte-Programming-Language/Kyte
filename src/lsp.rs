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

fn analyze_text(text: &str) -> Vec<Diagnostic> {
    let src = text.to_string();
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
            vec![Diagnostic {
                range: Range::default(),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("kyte".into()),
                message: format!("Parse error: {}", msg),
                ..Default::default()
            }]
        }
    }
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
        "int"    => "**int** — 64-bit signed integer type",
        "float"  => "**float** — 64-bit floating-point type",
        "string" => "**string** — UTF-8 string type",
        "bool"   => "**bool** — boolean type (`true` / `false`)",
        "fn" | "function" => "**fn** — declare a function",
        "vault"  => "**vault** — managed-memory declaration",
        "yield"  => "**yield** — output a value (print)",
        "kill"   => "**kill** — terminate the current anchor",
        "exit"   => "**exit** — exit the program",
        "return" => "**return** — return a value from a function",
        "if"     => "**if** — conditional branch",
        "else"   => "**else** — alternative branch",
        "loop"   => "**loop** — infinite loop (`break` to exit)",
        "for"    => "**for** — range-based loop\n\n```kyte\nfor i in 0..10 { … }\n```",
        "break"  => "**break** — exit the innermost loop",
        "true"   => "**true** — boolean literal",
        "false"  => "**false** — boolean literal",
        "free"   => "**free(name)** — release vault memory",
        _ => return None,
    };
    Some(s.into())
}

fn symbol_hover(text: &str, word: &str) -> Option<String> {
    let src = text.to_string();
    let r = catch_unwind(AssertUnwindSafe(|| -> Option<String> {
        let mut lex = Lexer::new(&src);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        let ast = par.parse();

        for (item, _) in &ast.items {
            if let TopLevel::Function { name, params, return_ty, .. } = item {
                if name == word {
                    let ps: Vec<String> = params
                        .iter()
                        .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                        .collect();
                    let ret = return_ty
                        .as_ref()
                        .map(|t| format!(" -> {}", ty_str(t)))
                        .unwrap_or_default();
                    return Some(format!("```kyte\nfn {}({}){}\n```", name, ps.join(", "), ret));
                }
            }
        }
        None
    }));
    r.ok().flatten()
}

fn ty_str(ty: &Ty) -> &'static str {
    match ty {
        Ty::Int => "int",
        Ty::Float => "float",
        Ty::String => "string",
        Ty::Bool => "bool",
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
    let mut lex = Lexer::new(src);
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
    ("int",    CompletionItemKind::TYPE_PARAMETER,  "64-bit integer"),
    ("float",  CompletionItemKind::TYPE_PARAMETER,  "64-bit float"),
    ("string", CompletionItemKind::TYPE_PARAMETER,  "String type"),
    ("bool",   CompletionItemKind::TYPE_PARAMETER,  "Boolean type"),
    ("if",     CompletionItemKind::KEYWORD,        "Conditional"),
    ("else",   CompletionItemKind::KEYWORD,        "Alternative branch"),
    ("for",    CompletionItemKind::KEYWORD,        "Range loop"),
    ("loop",   CompletionItemKind::KEYWORD,        "Infinite loop"),
    ("break",  CompletionItemKind::KEYWORD,        "Exit loop"),
    ("return", CompletionItemKind::KEYWORD,        "Return value"),
    ("yield",  CompletionItemKind::KEYWORD,        "Output value"),
    ("vault",  CompletionItemKind::KEYWORD,        "Managed memory"),
    ("free",   CompletionItemKind::KEYWORD,        "Release memory"),
    ("kill",   CompletionItemKind::KEYWORD,        "Terminate anchor"),
    ("exit",   CompletionItemKind::KEYWORD,        "Exit program"),
    ("true",   CompletionItemKind::CONSTANT,       "Boolean true"),
    ("false",  CompletionItemKind::CONSTANT,       "Boolean false"),
];

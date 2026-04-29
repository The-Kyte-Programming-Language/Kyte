use std::panic::{catch_unwind, AssertUnwindSafe};

use lsp_server::{Connection, Message, Notification};
use lsp_types::*;

use super::imports::preprocess_source_with_imports;
use crate::analyzer::{Analyzer, CompileError, Severity};
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn send_diagnostics(
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

pub(super) fn analyze_text(uri: &Uri, text: &str) -> Vec<Diagnostic> {
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

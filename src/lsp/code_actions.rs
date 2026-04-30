use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use std::collections::HashMap;

use super::diagnostics::analyze_text;

pub(super) fn compute_code_actions(
    text: &str,
    uri: &Uri,
    range: Range,
) -> Vec<CodeActionOrCommand> {
    let diags = analyze_text(uri, text);
    let mut actions: Vec<CodeActionOrCommand> = Vec::new();

    for diag in &diags {
        // 요청 범위와 겹치는 진단만 처리
        if !ranges_overlap(diag.range, range) {
            continue;
        }
        let code = diag
            .code
            .as_ref()
            .and_then(|c| match c {
                lsp_types::NumberOrString::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("");

        match code {
            // E004: 선언되지 않은 변수 → 선언 추가 quickfix
            "E004" => {
                if let Some(var_name) = extract_undeclared_var(&diag.message) {
                    let insert_line = diag.range.start.line;
                    let indent = indentation_at(text, insert_line as usize);
                    let new_text = format!("{}int {} = 0;\n", indent, var_name);
                    #[allow(clippy::mutable_key_type)]
                    let mut changes = HashMap::new();
                    changes.insert(
                        uri.clone(),
                        vec![TextEdit {
                            range: Range {
                                start: Position {
                                    line: insert_line,
                                    character: 0,
                                },
                                end: Position {
                                    line: insert_line,
                                    character: 0,
                                },
                            },
                            new_text,
                        }],
                    );
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Declare 'int {} = 0'", var_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        is_preferred: Some(true),
                        ..Default::default()
                    }));
                }
            }

            // E013: 미선언 함수 → 함수 스텁 생성
            "E013" => {
                if let Some(fn_name) = extract_undeclared_fn(&diag.message) {
                    let insert_line = find_insert_line_for_fn(text, diag.range.start.line);
                    let new_text = format!("\nfn {}() {{\n    // TODO\n}}\n", fn_name);
                    #[allow(clippy::mutable_key_type)]
                    let mut changes = HashMap::new();
                    changes.insert(
                        uri.clone(),
                        vec![TextEdit {
                            range: Range {
                                start: Position {
                                    line: insert_line,
                                    character: 0,
                                },
                                end: Position {
                                    line: insert_line,
                                    character: 0,
                                },
                            },
                            new_text,
                        }],
                    );
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Create function '{}'", fn_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        is_preferred: Some(false),
                        ..Default::default()
                    }));
                }
            }

            // W001: 미사용 변수 → 이름 앞에 _ 붙이기
            "W001" => {
                if let Some(var_name) = extract_unused_var(&diag.message) {
                    let new_name = format!("_{}", var_name);
                    let edits = rename_in_text(text, uri, &var_name, &new_name);
                    if let Some(edit) = edits {
                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("Rename to '{}' (suppress warning)", new_name),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diag.clone()]),
                            edit: Some(edit),
                            is_preferred: Some(false),
                            ..Default::default()
                        }));
                    }
                }
            }

            // E021: 잘못된 캐스트 → as 캐스트 제안 (정보 제공)
            "E002" | "E008" | "E009" | "E012" => {
                // 타입 불일치: 문서 링크를 action으로 제공할 수 있으나,
                // 일반적으로 자동 수정이 어려우므로 생략
            }

            _ => {}
        }
    }

    actions
}

// ── 헬퍼 ──────────────────────────────────────────────────────────────────

fn ranges_overlap(a: Range, b: Range) -> bool {
    a.start.line <= b.end.line && b.start.line <= a.end.line
}

fn extract_undeclared_var(msg: &str) -> Option<String> {
    // "Undeclared variable 'foo'"
    let after = msg.split('\'').nth(1)?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn extract_undeclared_fn(msg: &str) -> Option<String> {
    // "Undeclared function 'foo'" or "Undeclared method 'foo'"
    let after = msg.split('\'').nth(1)?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn extract_unused_var(msg: &str) -> Option<String> {
    // "Variable 'foo' is declared but never used"
    let after = msg.split('\'').nth(1)?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn indentation_at(text: &str, line_idx: usize) -> String {
    text.lines()
        .nth(line_idx)
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect())
        .unwrap_or_default()
}

fn find_insert_line_for_fn(text: &str, before_line: u32) -> u32 {
    // 현재 anchor/function 블록의 시작을 찾아 그 앞에 삽입
    // 간단하게: 앞으로 스캔하며 `@` 또는 `fn ` 시작하는 줄 찾기
    let lines: Vec<&str> = text.lines().collect();
    let mut line = before_line as usize;
    while line > 0 {
        let trimmed = lines.get(line).unwrap_or(&"").trim();
        if trimmed.starts_with('@') || trimmed.starts_with("fn ") {
            return line as u32;
        }
        line -= 1;
    }
    0
}

fn rename_in_text(text: &str, uri: &Uri, old: &str, new: &str) -> Option<WorkspaceEdit> {
    let edits: Vec<TextEdit> = text
        .lines()
        .enumerate()
        .flat_map(|(li, line)| {
            let mut col = 0usize;
            let mut result = Vec::new();
            while let Some(off) = line[col..].find(old) {
                let abs = col + off;
                let before_ok = abs == 0
                    || !line.as_bytes()[abs - 1].is_ascii_alphanumeric()
                        && line.as_bytes()[abs - 1] != b'_';
                let after_pos = abs + old.len();
                let after_ok = after_pos >= line.len()
                    || !line.as_bytes()[after_pos].is_ascii_alphanumeric()
                        && line.as_bytes()[after_pos] != b'_';
                if before_ok && after_ok {
                    result.push(TextEdit {
                        range: Range {
                            start: Position {
                                line: li as u32,
                                character: abs as u32,
                            },
                            end: Position {
                                line: li as u32,
                                character: after_pos as u32,
                            },
                        },
                        new_text: new.to_string(),
                    });
                }
                col = abs + old.len().max(1);
            }
            result
        })
        .collect();

    if edits.is_empty() {
        return None;
    }
    #[allow(clippy::mutable_key_type)]
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

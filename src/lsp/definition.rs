use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};

use lsp_types::{GotoDefinitionResponse, Location, Position, Range, Uri};

use super::imports::{parse_import_path, preprocess_source, uri_to_file_path};
use super::util::word_at;
use crate::ast::TopLevel;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_definition(
    text: &str,
    pos: Position,
    uri: &Uri,
) -> Option<GotoDefinitionResponse> {
    let word = word_at(text, pos)?;
    let src = preprocess_source(text);
    let lines: Vec<&str> = text.lines().collect();

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
                TopLevel::Trait { name, .. } if *name == word => true,
                TopLevel::Module { name, .. } if *name == word => true,
                _ => false,
            };
            if found {
                let loc = make_location(uri, &lines, span.line, &word);
                return Some(loc);
            }
            if let TopLevel::Anchor { children, .. } = item {
                if let Some(loc) = find_def_in_children(children, &word, uri, &lines) {
                    return Some(loc);
                }
            }
        }

        // 로컬 변수 선언 탐색: `type name =` 또는 `auto name =`
        if let Some(loc) = find_local_var_def(text, &word, uri) {
            return Some(loc);
        }

        None
    }));

    // cross-file: import 파일에서 탐색
    if let Ok(None) = r {
        if let Some(loc) = find_in_imports(text, uri, &word) {
            return Some(GotoDefinitionResponse::Scalar(loc));
        }
    }

    r.ok().flatten().map(GotoDefinitionResponse::Scalar)
}

fn make_location(uri: &Uri, lines: &[&str], span_line: usize, name: &str) -> Location {
    let line_0 = span_line.saturating_sub(1) as u32;
    let col = lines
        .get(line_0 as usize)
        .and_then(|l| l.find(name).map(|c| c as u32))
        .unwrap_or(0);
    Location {
        uri: uri.clone(),
        range: Range {
            start: Position {
                line: line_0,
                character: col,
            },
            end: Position {
                line: line_0,
                character: col + name.len() as u32,
            },
        },
    }
}

fn find_def_in_children(
    children: &[(TopLevel, crate::ast::Span)],
    word: &str,
    uri: &Uri,
    lines: &[&str],
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
            return Some(make_location(uri, lines, span.line, word));
        }
        if let TopLevel::Anchor {
            children: nested, ..
        } = item
        {
            if let Some(loc) = find_def_in_children(nested, word, uri, lines) {
                return Some(loc);
            }
        }
    }
    None
}

/// 소스 내 로컬 변수 선언 위치 탐색
/// `int name = ...` / `auto name = ...` / `string name = ...`
fn find_local_var_def(text: &str, name: &str, uri: &Uri) -> Option<Location> {
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        // 각 타입 뒤에 공백 + name이 등장하는지 확인
        for prefix in &[
            "int ", "float ", "string ", "bool ", "auto ", "i8 ", "i16 ", "i32 ", "i64 ", "u8 ",
            "u16 ", "u32 ", "u64 ", "Vault ",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let var_name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if var_name == name {
                    let col = line.find(name).unwrap_or(0) as u32;
                    return Some(Location {
                        uri: uri.clone(),
                        range: Range {
                            start: Position {
                                line: line_idx as u32,
                                character: col,
                            },
                            end: Position {
                                line: line_idx as u32,
                                character: col + name.len() as u32,
                            },
                        },
                    });
                }
            }
        }
        // `for i in ...` 패턴
        if let Some(rest) = trimmed.strip_prefix("for ") {
            let var_name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if var_name == name {
                let col = line.find(name).unwrap_or(0) as u32;
                return Some(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: Position {
                            line: line_idx as u32,
                            character: col,
                        },
                        end: Position {
                            line: line_idx as u32,
                            character: col + name.len() as u32,
                        },
                    },
                });
            }
        }
    }
    None
}

/// import된 파일에서 심볼 탐색
fn find_in_imports(text: &str, uri: &Uri, word: &str) -> Option<Location> {
    let root_path = uri_to_file_path(uri)?;
    let base_dir = root_path.parent()?;

    for line in text.lines() {
        if let Some(rel) = parse_import_path(line) {
            let import_path = base_dir.join(&rel);
            let import_src = fs::read_to_string(&import_path).ok()?;
            let import_lines: Vec<&str> = import_src.lines().collect();
            let preprocessed = super::imports::preprocess_source(&import_src);

            let r = catch_unwind(AssertUnwindSafe(|| -> Option<Location> {
                let tokens = Lexer::new(&preprocessed).tokenize();
                let ast = Parser::new(tokens).parse();
                let import_uri: Uri = format!(
                    "file:///{}",
                    import_path.to_string_lossy().replace('\\', "/")
                )
                .parse()
                .ok()?;

                for (item, span) in &ast.items {
                    let found = match item {
                        TopLevel::Function { name, .. } if *name == word => true,
                        TopLevel::Struct { name, .. } if *name == word => true,
                        TopLevel::Enum { name, .. } if *name == word => true,
                        _ => false,
                    };
                    if found {
                        return Some(make_location(&import_uri, &import_lines, span.line, word));
                    }
                }
                None
            }));
            if let Ok(Some(loc)) = r {
                return Some(loc);
            }
        }
    }
    None
}

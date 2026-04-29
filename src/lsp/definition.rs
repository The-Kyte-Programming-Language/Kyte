use std::panic::{catch_unwind, AssertUnwindSafe};

use lsp_types::{GotoDefinitionResponse, Location, Position, Range, Uri};

use super::imports::preprocess_source;
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

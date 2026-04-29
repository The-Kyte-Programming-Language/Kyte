use lsp_types::{Location, Position, Range, SymbolInformation, SymbolKind, Uri};

use super::imports::preprocess_source;
use crate::ast::TopLevel;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_document_symbols(text: &str, uri: &Uri) -> Vec<SymbolInformation> {
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

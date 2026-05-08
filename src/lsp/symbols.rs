use lsp_types::{DocumentSymbol, DocumentSymbolResponse, Position, Range, SymbolKind, Uri};

use super::imports::preprocess_source;
use super::util::ty_str;
use crate::ast::{AnchorKind, TopLevel};
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_document_symbols(text: &str, _uri: &Uri) -> DocumentSymbolResponse {
    let src = preprocess_source(text);
    let tokens = Lexer::new(&src).tokenize();
    let ast = Parser::new(tokens, &src).parse();
    let lines: Vec<&str> = text.lines().collect();

    let mut syms: Vec<DocumentSymbol> = Vec::new();
    for (item, span) in &ast.items {
        if let Some(sym) = build_symbol(item, span, &lines) {
            syms.push(sym);
        }
    }
    DocumentSymbolResponse::Nested(syms)
}

fn line_range(lines: &[&str], line_1: usize) -> Range {
    let ln = line_1.saturating_sub(1) as u32;
    let len = lines
        .get(ln as usize)
        .map(|l| l.len() as u32)
        .unwrap_or(1)
        .max(1);
    Range {
        start: Position {
            line: ln,
            character: 0,
        },
        end: Position {
            line: ln,
            character: len,
        },
    }
}

fn name_range(lines: &[&str], line_1: usize, name: &str) -> Range {
    let ln = line_1.saturating_sub(1) as u32;
    let col = lines
        .get(ln as usize)
        .and_then(|l| l.find(name).map(|c| c as u32))
        .unwrap_or(0);
    Range {
        start: Position {
            line: ln,
            character: col,
        },
        end: Position {
            line: ln,
            character: col + name.len() as u32,
        },
    }
}

fn build_symbol(
    item: &TopLevel,
    span: &crate::ast::Span,
    lines: &[&str],
) -> Option<DocumentSymbol> {
    #[allow(deprecated)]
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
            Some(DocumentSymbol {
                name: name.clone(),
                detail: Some(format!("fn({}){}", ps.join(", "), ret)),
                kind: SymbolKind::FUNCTION,
                tags: None,
                deprecated: None,
                range: line_range(lines, span.line),
                selection_range: name_range(lines, span.line, name),
                children: None,
            })
        }

        TopLevel::Struct { name, fields } => {
            let children: Vec<DocumentSymbol> = fields
                .iter()
                .map(|f| DocumentSymbol {
                    name: f.name.clone(),
                    detail: Some(ty_str(&f.ty)),
                    kind: SymbolKind::FIELD,
                    tags: None,
                    deprecated: None,
                    range: line_range(lines, span.line),
                    selection_range: line_range(lines, span.line),
                    children: None,
                })
                .collect();
            Some(DocumentSymbol {
                name: name.clone(),
                detail: Some(format!("struct {}", name)),
                kind: SymbolKind::STRUCT,
                tags: None,
                deprecated: None,
                range: line_range(lines, span.line),
                selection_range: name_range(lines, span.line, name),
                children: Some(children),
            })
        }

        TopLevel::Enum { name, variants } => {
            let children: Vec<DocumentSymbol> = variants
                .iter()
                .map(|v| DocumentSymbol {
                    name: v.name.clone(),
                    detail: v.ty.as_ref().map(|t| ty_str(t)),
                    kind: SymbolKind::ENUM_MEMBER,
                    tags: None,
                    deprecated: None,
                    range: line_range(lines, span.line),
                    selection_range: line_range(lines, span.line),
                    children: None,
                })
                .collect();
            Some(DocumentSymbol {
                name: name.clone(),
                detail: Some(format!("enum {}", name)),
                kind: SymbolKind::ENUM,
                tags: None,
                deprecated: None,
                range: line_range(lines, span.line),
                selection_range: name_range(lines, span.line, name),
                children: Some(children),
            })
        }

        TopLevel::Trait { name, methods } => {
            let children: Vec<DocumentSymbol> = methods
                .iter()
                .map(|m| {
                    let ps: Vec<String> = m
                        .params
                        .iter()
                        .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                        .collect();
                    DocumentSymbol {
                        name: m.name.clone(),
                        detail: Some(format!("fn({})", ps.join(", "))),
                        kind: SymbolKind::METHOD,
                        tags: None,
                        deprecated: None,
                        range: line_range(lines, span.line),
                        selection_range: line_range(lines, span.line),
                        children: None,
                    }
                })
                .collect();
            Some(DocumentSymbol {
                name: name.clone(),
                detail: Some(format!("trait {}", name)),
                kind: SymbolKind::INTERFACE,
                tags: None,
                deprecated: None,
                range: line_range(lines, span.line),
                selection_range: name_range(lines, span.line, name),
                children: Some(children),
            })
        }

        TopLevel::Module { name, items } => {
            let children: Vec<DocumentSymbol> = items
                .iter()
                .filter_map(|(i, s)| build_symbol(i, s, lines))
                .collect();
            Some(DocumentSymbol {
                name: name.clone(),
                detail: Some(format!("mod {}", name)),
                kind: SymbolKind::MODULE,
                tags: None,
                deprecated: None,
                range: line_range(lines, span.line),
                selection_range: name_range(lines, span.line, name),
                children: Some(children),
            })
        }

        TopLevel::Anchor { name, kind, .. } => {
            let kind_str = match kind {
                AnchorKind::Main => "main".to_string(),
                AnchorKind::Plain => "plain".to_string(),
                AnchorKind::Thread => "thread".to_string(),
                AnchorKind::Event(e) => format!("event({})", e),
            };
            Some(DocumentSymbol {
                name: name.clone(),
                detail: Some(format!("@{}({})", name, kind_str)),
                kind: SymbolKind::EVENT,
                tags: None,
                deprecated: None,
                range: line_range(lines, span.line),
                selection_range: name_range(lines, span.line, name),
                children: None,
            })
        }

        TopLevel::ConstDecl { ty, name, .. } => Some(DocumentSymbol {
            name: name.clone(),
            detail: Some(format!("const {}", ty_str(ty))),
            kind: SymbolKind::CONSTANT,
            tags: None,
            deprecated: None,
            range: line_range(lines, span.line),
            selection_range: name_range(lines, span.line, name),
            children: None,
        }),

        TopLevel::Impl {
            trait_name,
            target_ty,
            methods,
        } => {
            let children: Vec<DocumentSymbol> = methods
                .iter()
                .filter_map(|(i, s)| build_symbol(i, s, lines))
                .collect();
            Some(DocumentSymbol {
                name: format!("impl {} for {}", trait_name, target_ty),
                detail: None,
                kind: SymbolKind::CLASS,
                tags: None,
                deprecated: None,
                range: line_range(lines, span.line),
                selection_range: name_range(lines, span.line, target_ty),
                children: Some(children),
            })
        }

        TopLevel::ExternFn { .. } => None, // extern declarations not shown as symbols
    }
}

use std::collections::HashMap;

use lsp_server::{Connection, Message};
use lsp_types::*;

#[path = "lsp/code_actions.rs"]
mod code_actions;
#[path = "lsp/completion.rs"]
mod completion;
#[path = "lsp/definition.rs"]
mod definition;
#[path = "lsp/diagnostics.rs"]
mod diagnostics;
#[path = "lsp/dispatch.rs"]
mod dispatch;
#[path = "lsp/hover.rs"]
mod hover;
#[path = "lsp/imports.rs"]
mod imports;
#[path = "lsp/references.rs"]
mod references;
#[path = "lsp/rename.rs"]
mod rename;
#[path = "lsp/signature.rs"]
mod signature;
#[path = "lsp/symbols.rs"]
mod symbols;
#[path = "lsp/util.rs"]
mod util;

use dispatch::{dispatch_notification, dispatch_request};

pub fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    eprintln!("[kyte-lsp] starting …");

    let (conn, io) = Connection::stdio();

    let caps = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), "@".into(), "(".into()]),
            resolve_provider: Some(false),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![",".into()]),
            ..Default::default()
        }),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![
                CodeActionKind::QUICKFIX,
                CodeActionKind::REFACTOR,
            ]),
            resolve_provider: Some(false),
            ..Default::default()
        })),
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

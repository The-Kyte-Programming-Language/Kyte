use std::collections::HashMap;

use lsp_server::{Connection, Message, Notification, Response};
use lsp_types::*;

use super::code_actions::compute_code_actions;
use super::completion::compute_completions;
use super::definition::compute_definition;
use super::diagnostics::send_diagnostics;
use super::hover::compute_hover;
use super::references::compute_references;
use super::rename::compute_rename;
use super::signature::compute_signature_help;
use super::symbols::compute_document_symbols;

#[allow(clippy::mutable_key_type)]
pub(super) fn dispatch_notification(
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

#[allow(clippy::mutable_key_type)]
pub(super) fn dispatch_request(
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
            let pos = p.text_document_position.position;
            let list = compute_completions(uri, docs.get(uri).map(|s| s.as_str()), pos);
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
            let result = docs
                .get(uri)
                .and_then(|t| compute_rename(t, pos, uri, &p.new_name));
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

        "textDocument/codeAction" => {
            let p: CodeActionParams = serde_json::from_value(req.params.clone())?;
            let uri = &p.text_document.uri;
            let range = p.range;
            let result: Vec<CodeActionOrCommand> = docs
                .get(uri)
                .map(|t| compute_code_actions(t, uri, range))
                .unwrap_or_default();
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

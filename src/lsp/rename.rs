use std::collections::HashMap;

use lsp_types::{Position, TextEdit, Uri, WorkspaceEdit};

use super::references::compute_references;
use super::util::word_at;

pub(super) fn compute_rename(
    text: &str,
    pos: Position,
    uri: &Uri,
    new_name: &str,
) -> Option<WorkspaceEdit> {
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

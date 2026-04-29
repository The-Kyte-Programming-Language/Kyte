use lsp_types::{Location, Position, Range, Uri};

use super::util::word_at;

pub(super) fn compute_references(text: &str, pos: Position, uri: &Uri) -> Vec<Location> {
    let word = match word_at(text, pos) {
        Some(w) => w,
        None => return vec![],
    };
    // Simple text-based search: find all occurrences of the word as a whole word
    let mut refs = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let mut col = 0usize;
        let hay = line;
        while let Some(offset) = hay[col..].find(&word) {
            let abs = col + offset;
            // Ensure it's a whole word
            let before_ok = abs == 0 || {
                let c = hay.as_bytes()[abs - 1];
                !(c.is_ascii_alphanumeric() || c == b'_')
            };
            let after_pos = abs + word.len();
            let after_ok = after_pos >= hay.len() || {
                let c = hay.as_bytes()[after_pos];
                !(c.is_ascii_alphanumeric() || c == b'_')
            };
            if before_ok && after_ok {
                refs.push(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: Position {
                            line: line_idx as u32,
                            character: abs as u32,
                        },
                        end: Position {
                            line: line_idx as u32,
                            character: after_pos as u32,
                        },
                    },
                });
            }
            col = abs + word.len().max(1);
        }
    }
    refs
}

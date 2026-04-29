use lsp_types::Position;

use crate::ast::Ty;

/// Extract the word under the cursor
pub(super) fn word_at(text: &str, pos: Position) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let col = pos.character as usize;
    if col >= chars.len() || !(chars[col].is_alphanumeric() || chars[col] == '_') {
        return None;
    }
    let mut lo = col;
    while lo > 0 && (chars[lo - 1].is_alphanumeric() || chars[lo - 1] == '_') {
        lo -= 1;
    }
    let mut hi = col;
    while hi < chars.len() && (chars[hi].is_alphanumeric() || chars[hi] == '_') {
        hi += 1;
    }
    Some(chars[lo..hi].iter().collect())
}

pub(super) fn format_with_doc(sig: &str, doc: &str) -> String {
    if doc.is_empty() {
        sig.to_string()
    } else {
        // doc 안의 ```kyte 를 ``` 로 통일 (VS Code 호버에서 인식 보장)
        let doc_normalized = doc.replace("```kyte", "```");
        format!("{}\n\n---\n\n{}", sig, doc_normalized)
    }
}

/// `fn` 선언(1-indexed `fn_line`) 바로 위에 있는 연속된 `///` 주석을 추출한다.
/// 들여쓰기(4칸+)된 줄은 자동으로 코드 블록으로 감싼다.
pub(super) fn extract_doc_comment(src: &str, fn_line: usize) -> String {
    let lines: Vec<&str> = src.lines().collect();
    if fn_line == 0 || fn_line > lines.len() {
        return String::new();
    }
    // fn_line은 1-indexed → 배열에선 fn_line-1.  그 바로 위부터 위로 스캔
    let mut doc_lines: Vec<&str> = Vec::new();
    let mut idx = fn_line as isize - 2; // 바로 윗줄(0-indexed)
    while idx >= 0 {
        let trimmed = lines[idx as usize].trim();
        if let Some(rest) = trimmed.strip_prefix("///") {
            doc_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
            idx -= 1;
        } else {
            break;
        }
    }
    doc_lines.reverse();

    // 들여쓰기(4칸+)된 줄을 자동으로 ```kyte 코드 블록으로 감싸기
    let mut result = String::new();
    let mut in_code = false;
    let mut in_user_fence = false;

    for line in &doc_lines {
        // 사용자가 직접 ``` 를 쓴 경우 그대로 통과
        if line.trim().starts_with("```") {
            in_user_fence = !in_user_fence;
            result.push_str(line);
            result.push('\n');
            continue;
        }
        if in_user_fence {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        let is_code_line = line.starts_with("    ") || line.starts_with('\t');
        if is_code_line && !in_code {
            result.push_str("```\n");
            in_code = true;
        } else if !is_code_line && in_code {
            result.push_str("```\n");
            in_code = false;
        }

        if in_code {
            // 들여쓰기 4칸 제거
            let stripped = if let Some(s) = line.strip_prefix("    ") {
                s
            } else if let Some(s) = line.strip_prefix('\t') {
                s
            } else {
                line
            };
            result.push_str(stripped);
            result.push('\n');
        } else {
            result.push_str(line);
            // Markdown에서 줄바꿈 유지 + 한 줄 여백으로 가독성 개선
            result.push_str("  \n\n");
        }
    }
    if in_code {
        result.push_str("```\n");
    }

    result.trim_end().to_string()
}

pub(super) fn ty_str(ty: &Ty) -> String {
    match ty {
        Ty::Int => "int".to_string(),
        Ty::Float => "float".to_string(),
        Ty::String => "string".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::I8 => "i8".to_string(),
        Ty::I16 => "i16".to_string(),
        Ty::I32 => "i32".to_string(),
        Ty::I64 => "i64".to_string(),
        Ty::U8 => "u8".to_string(),
        Ty::U16 => "u16".to_string(),
        Ty::U32 => "u32".to_string(),
        Ty::U64 => "u64".to_string(),
        Ty::Array(inner) => format!("{}[]", ty_str(inner)),
        Ty::Struct(name) => name.clone(),
        Ty::Auto => "auto".to_string(),
        Ty::Enum(name) => name.clone(),
        Ty::TypeParam(name) => name.clone(),
        Ty::Fn(params, ret) => {
            let ps: Vec<String> = params.iter().map(ty_str).collect();
            let ret_s = ret
                .as_deref()
                .map(ty_str)
                .unwrap_or_else(|| "void".to_string());
            format!("fn({}) -> {}", ps.join(", "), ret_s)
        }
    }
}

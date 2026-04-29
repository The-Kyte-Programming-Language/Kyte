use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use lsp_types::Uri;

pub(super) fn is_import_line(line: &str) -> bool {
    parse_import_path(line).is_some()
}

pub(super) fn preprocess_source(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        if is_import_line(line) {
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

pub(super) fn append_non_import_lines(src: &str, out: &mut String) {
    for line in src.lines() {
        if is_import_line(line) {
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
}

pub(super) fn visit_import_file(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut String) {
    // canonicalize 실패해도(Windows에서 흔함) 원본 경로로 폴백
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    if !seen.insert(resolved.clone()) {
        return;
    }

    let text = match fs::read_to_string(&resolved).or_else(|_| fs::read_to_string(path)) {
        Ok(t) => t,
        Err(_) => return,
    };

    let base_dir = resolved
        .parent()
        .or_else(|| path.parent())
        .unwrap_or_else(|| Path::new("."));

    for line in text.lines() {
        if let Some(rel) = parse_import_path(line) {
            visit_import_file(&base_dir.join(rel), seen, out);
        }
    }

    out.push_str(&format!(
        "\n// ---- import file: {} ----\n",
        resolved.display()
    ));
    append_non_import_lines(&text, out);
}

pub(super) fn parse_import_path(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with("import") {
        return None;
    }
    let rest = t["import".len()..].trim_start();
    let raw_path = rest.strip_suffix(';')?.trim();
    if raw_path.is_empty() {
        return None;
    }
    if raw_path.starts_with('"') && raw_path.ends_with('"') && raw_path.len() >= 2 {
        return Some(raw_path[1..raw_path.len() - 1].to_string());
    }
    Some(raw_path.to_string())
}

pub(super) fn uri_to_file_path(uri: &Uri) -> Option<PathBuf> {
    let raw = uri.to_string();
    if !raw.starts_with("file://") {
        return None;
    }
    let mut path = raw.trim_start_matches("file://").to_string();
    // URL 디코딩
    path = path.replace("%20", " ");
    path = path.replace("%3A", ":").replace("%3a", ":");
    // Windows file URI: file:///C:/... -> /C:/... -> C:/...
    // 슬래시로 시작하고 두 번째 문자 뒤에 콜론이 있으면 Windows 드라이브 경로
    if path.starts_with('/') {
        let rest = &path[1..];
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
            path = rest.to_string();
        }
    }
    // 백슬래시 정규화 (혹시 섞여있을 경우)
    path = path.replace('/', std::path::MAIN_SEPARATOR_STR);
    Some(PathBuf::from(path))
}

pub(super) fn preprocess_source_with_imports(uri: &Uri, text: &str) -> (String, usize, usize) {
    let own_line_count = text.lines().count();
    let mut merged = String::new();

    let root_path = match uri_to_file_path(uri) {
        Some(p) => p,
        None => return (preprocess_source(text), 0, own_line_count),
    };
    let base_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
    let mut seen = HashSet::new();
    if let Ok(canon) = fs::canonicalize(&root_path) {
        seen.insert(canon);
    }

    // import 파일을 먼저 — 현재 파일보다 앞에 붙여야
    // analyzer가 함수 선언을 사용 전에 볼 수 있음
    for line in text.lines() {
        if let Some(rel) = parse_import_path(line) {
            visit_import_file(&base_dir.join(rel), &mut seen, &mut merged);
        }
    }

    // import 파일들이 차지하는 라인 수
    let import_line_offset = merged.lines().count();

    // 현재 파일은 나중에
    append_non_import_lines(text, &mut merged);

    (merged, import_line_offset, own_line_count)
}

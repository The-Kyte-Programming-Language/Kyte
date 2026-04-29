use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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

pub(super) fn load_source_with_imports(entry: &str) -> Result<String, String> {
    fn visit(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut String) -> Result<(), String> {
        let canonical =
            fs::canonicalize(path).map_err(|e| format!("{} ({})", path.display(), e))?;
        if seen.contains(&canonical) {
            return Ok(());
        }
        seen.insert(canonical.clone());

        let text = fs::read_to_string(&canonical)
            .map_err(|e| format!("{} ({})", canonical.display(), e))?;
        let base_dir = canonical.parent().unwrap_or_else(|| Path::new("."));

        for line in text.lines() {
            if let Some(rel) = parse_import_path(line) {
                let dep = base_dir.join(rel);
                visit(&dep, seen, out)?;
            }
        }

        out.push_str(&format!("\n// ---- file: {} ----\n", canonical.display()));
        for line in text.lines() {
            if parse_import_path(line).is_none() {
                out.push_str(line);
                out.push('\n');
            }
        }
        Ok(())
    }

    let mut seen = HashSet::new();
    let mut merged = String::new();
    visit(Path::new(entry), &mut seen, &mut merged)?;
    Ok(merged)
}

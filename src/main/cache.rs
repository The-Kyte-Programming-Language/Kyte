use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Compute a fast hash of a source string (used as cache key).
pub(super) fn hash_source(source: &str) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

/// Directory where cached object files are stored.
pub(super) fn cache_dir() -> PathBuf {
    PathBuf::from(".kyte-build")
}

/// Path for the cached object file corresponding to a source hash.
pub(super) fn cached_obj_path(source_hash: u64) -> PathBuf {
    cache_dir().join(format!("{:016x}.o", source_hash))
}

/// Try to serve a build from cache.
///
/// If `.kyte-build/<hash>.o` exists, copies it to `output_obj` and returns
/// `true`.  Returns `false` when the cache is cold (miss).
pub(super) fn try_cache_hit(source_hash: u64, output_obj: &str) -> bool {
    let cached = cached_obj_path(source_hash);
    if !cached.exists() {
        return false;
    }
    match fs::copy(&cached, output_obj) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("  [cache] copy failed: {}", e);
            false
        }
    }
}

/// Save a freshly compiled object file to the cache.
pub(super) fn save_to_cache(source_hash: u64, obj_path: &str) {
    if let Err(e) = fs::create_dir_all(cache_dir()) {
        eprintln!("  [cache] cannot create cache dir: {}", e);
        return;
    }
    let cached = cached_obj_path(source_hash);
    if let Err(e) = fs::copy(obj_path, &cached) {
        eprintln!("  [cache] save failed: {}", e);
    }
}

/// Evict stale entries: remove cached .o files whose hash no longer matches
/// any file in the current workspace.  Call occasionally to reclaim disk.
pub(super) fn evict_stale(current_hash: u64) {
    let dir = cache_dir();
    let current_name = format!("{:016x}.o", current_hash);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".o") && name_str != current_name {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Returns the path to use for caching based on the input label.
pub(super) fn obj_path_for(label: &str) -> String {
    if label.ends_with(".ky") {
        label.replace(".ky", ".o")
    } else {
        "output.o".to_string()
    }
}

/// Verify the cache directory exists and is writable.
/// Creates it lazily if missing.  Returns `Ok(path)` on success.
pub(super) fn ensure_cache_dir() -> Result<PathBuf, String> {
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create cache dir: {}", e))?;
    Ok(dir)
}

pub(super) fn cached_ir_path(source_hash: u64) -> PathBuf {
    cache_dir().join(format!("{:016x}.ll", source_hash))
}

pub(super) fn save_ir_to_cache(source_hash: u64, ir_path: &str) {
    let _ = ensure_cache_dir();
    let cached = cached_ir_path(source_hash);
    let _ = fs::copy(ir_path, &cached);
}

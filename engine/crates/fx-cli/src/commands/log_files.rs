use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn find_log_files(logs_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !logs_dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(logs_dir)? {
        let path = entry?.path();
        if is_log_file(&path) {
            files.push(path);
        }
    }
    files.sort_by_key(file_sort_key);
    files.reverse();
    Ok(files)
}

fn is_log_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("log")
}

fn file_sort_key(path: &PathBuf) -> (std::time::SystemTime, String) {
    let modified = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    (modified, path.display().to_string())
}

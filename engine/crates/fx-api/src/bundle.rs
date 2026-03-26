use std::path::{Path, PathBuf};

/// Resolve the server binary path.
///
/// Inside a .app bundle: `Fawx.app/Contents/MacOS/fawx-server`
/// Outside a bundle (CLI): `std::env::current_exe()`
pub fn server_binary_path() -> Result<PathBuf, String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot determine executable path: {e}"))?;

    if let Some(bundle_path) = find_bundle_root(&exe) {
        let server_binary = bundle_path
            .join("Contents")
            .join("MacOS")
            .join("fawx-server");
        if server_binary.exists() {
            return Ok(server_binary);
        }

        let exe_name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("fawx");
        let alt = bundle_path.join("Contents").join("MacOS").join(exe_name);
        if alt.exists() {
            return Ok(alt);
        }
    }

    Ok(exe)
}

/// Walk up from the exe path looking for a .app directory
fn find_bundle_root(exe: &Path) -> Option<PathBuf> {
    let mut current = exe.parent()?;
    for _ in 0..5 {
        if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".app") {
                return Some(current.to_path_buf());
            }
        }
        current = current.parent()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_bundle_root_returns_none_for_non_bundle_path() {
        let path = Path::new("/usr/local/bin/fawx");
        assert!(find_bundle_root(path).is_none());
    }

    #[test]
    fn find_bundle_root_finds_app_directory() {
        let path = Path::new("/Applications/Fawx.app/Contents/MacOS/fawx");
        let root = find_bundle_root(path);
        assert_eq!(root, Some(PathBuf::from("/Applications/Fawx.app")));
    }

    #[test]
    fn find_bundle_root_finds_nested_app() {
        let path = Path::new("/Applications/Fawx.app/Contents/MacOS/fawx-server");
        let root = find_bundle_root(path);
        assert_eq!(root, Some(PathBuf::from("/Applications/Fawx.app")));
    }

    #[test]
    fn server_binary_path_returns_current_exe_outside_bundle() {
        let path = server_binary_path().expect("should resolve");
        assert!(path.exists());
    }
}

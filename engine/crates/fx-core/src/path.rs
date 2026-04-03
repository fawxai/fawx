use std::path::PathBuf;

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Expand a leading `~` or `~/` prefix to the user's home directory.
///
/// Only expands `~` at the very start of the path. `~user` and other strings
/// are returned unchanged.
#[must_use]
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    } else if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::{expand_tilde, home_dir};
    use std::path::PathBuf;

    #[test]
    fn expand_tilde_expands_home_prefix() {
        let result = expand_tilde("~/foo");
        let home = home_dir().expect("home dir");

        assert_eq!(result, home.join("foo"));
    }

    #[test]
    fn expand_tilde_expands_bare_home() {
        let result = expand_tilde("~");
        let home = home_dir().expect("home dir");

        assert_eq!(result, home);
    }

    #[test]
    fn expand_tilde_leaves_other_paths_unchanged() {
        assert_eq!(
            expand_tilde("/absolute/path"),
            PathBuf::from("/absolute/path")
        );
        assert_eq!(
            expand_tilde("relative/path"),
            PathBuf::from("relative/path")
        );
        assert_eq!(
            expand_tilde("~otheruser/foo"),
            PathBuf::from("~otheruser/foo")
        );
    }
}

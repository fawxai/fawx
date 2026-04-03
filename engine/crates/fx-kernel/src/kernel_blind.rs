const KERNEL_BLIND_PATH_PREFIXES: &[&str] = &[
    "engine/crates/fx-kernel/",
    "engine/crates/fx-auth/",
    "engine/crates/fx-security/",
    "engine/crates/fx-consensus/",
    "fawx-ripcord/",
    "tests/invariant/",
];

const READ_COMMAND_PREFIXES: &[&str] = &["cat ", "head ", "tail ", "less ", "more ", "bat "];
const SEARCH_COMMAND_PREFIXES: &[&str] = &["grep ", "rg ", "ag ", "find "];
const GIT_COMMAND_PREFIXES: &[&str] = &["git show ", "git log -p", "git diff ", "git blame "];
const RE_COMMAND_PREFIXES: &[&str] = &[
    "strings ", "objdump ", "otool ", "nm ", "readelf ", "hexdump ", "xxd ",
];

pub(crate) fn is_kernel_blind_path(relative_path: &str) -> bool {
    let normalized = normalize_relative_path(relative_path);
    KERNEL_BLIND_PATH_PREFIXES
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

#[must_use]
pub(crate) fn is_kernel_blind_enforced() -> bool {
    cfg!(feature = "kernel-blind")
}

pub(crate) fn shell_targets_kernel_path(command: &str) -> bool {
    command_targets_kernel_procfs(command)
        || command_targets_kernel_path(command, READ_COMMAND_PREFIXES)
        || command_targets_kernel_path(command, SEARCH_COMMAND_PREFIXES)
        || command_targets_kernel_path(command, GIT_COMMAND_PREFIXES)
        || command_targets_kernel_path(command, RE_COMMAND_PREFIXES)
}

pub(crate) fn normalize_relative_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let stripped = unified.strip_prefix("./").unwrap_or(&unified);
    let stripped = stripped.strip_prefix('/').unwrap_or(stripped);
    let mut parts = Vec::new();
    for segment in stripped.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    parts.join("/")
}

fn command_targets_kernel_procfs(command: &str) -> bool {
    command.contains("/proc/self/exe") || command.contains("/proc/self/maps")
}

fn command_targets_kernel_path(command: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| command.contains(prefix))
        && KERNEL_BLIND_PATH_PREFIXES
            .iter()
            .any(|path| command.contains(path))
}

#[cfg(test)]
mod tests {
    use super::{
        is_kernel_blind_enforced, is_kernel_blind_path, normalize_relative_path,
        shell_targets_kernel_path,
    };

    #[test]
    fn path_matching_handles_variants() {
        assert!(is_kernel_blind_path("engine/crates/fx-kernel/src/lib.rs"));
        assert!(is_kernel_blind_path(
            "./engine/crates/fx-auth/src/crypto/keys.rs"
        ));
        assert!(is_kernel_blind_path(
            "engine\\crates\\fx-security\\src\\audit\\mod.rs"
        ));
        assert!(!is_kernel_blind_path("docs/specs/kernel-blindness.md"));
    }

    #[test]
    fn shell_and_path_detection_share_kernel_blind_prefixes() {
        assert!(shell_targets_kernel_path(
            "cat engine/crates/fx-kernel/src/lib.rs"
        ));
        assert!(shell_targets_kernel_path(
            "rg TODO tests/invariant/tier3_test.rs"
        ));
        assert!(shell_targets_kernel_path(
            "git diff fawx-ripcord/src/main.rs"
        ));
        assert!(!shell_targets_kernel_path(
            "cat docs/specs/kernel-blindness.md"
        ));
    }

    #[test]
    fn normalize_relative_path_handles_variants() {
        assert_eq!(normalize_relative_path("./foo/bar"), "foo/bar");
        assert_eq!(normalize_relative_path("a/../b/c"), "b/c");
        assert_eq!(normalize_relative_path("/absolute/path"), "absolute/path");
        assert_eq!(
            normalize_relative_path("engine/../engine/crates/fx-kernel/src/lib.rs"),
            "engine/crates/fx-kernel/src/lib.rs"
        );
        assert_eq!(normalize_relative_path("a/./b/../c"), "a/c");
        assert_eq!(normalize_relative_path("foo\\bar\\baz"), "foo/bar/baz");
    }

    #[test]
    fn kernel_blind_enforcement_flag_matches_feature() {
        assert_eq!(is_kernel_blind_enforced(), cfg!(feature = "kernel-blind"));
    }
}

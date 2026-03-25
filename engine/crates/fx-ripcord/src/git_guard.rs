const ALL_BRANCHES_TARGET: &str = "*";

/// Check whether any push targets are protected branches.
/// Returns Err with a user-facing message listing blocked branches.
#[must_use = "the push guard result must be checked before running git push"]
pub fn check_push_allowed(targets: &[String], protected_branches: &[String]) -> Result<(), String> {
    let blocked = blocked_branches(targets, protected_branches);
    if blocked.is_empty() {
        return Ok(());
    }
    Err(format_blocked_push(&blocked))
}

/// Extract target branches from a shell command string.
/// Returns empty vec if the command is not a git push or targets can't be determined.
pub fn extract_push_targets(command: &str) -> Vec<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    match push_refspecs(&tokens) {
        Some(PushTargets::Refspecs(refspecs)) => {
            refspecs.into_iter().filter_map(normalize_target).collect()
        }
        Some(PushTargets::AllBranches) => vec![ALL_BRANCHES_TARGET.to_string()],
        Some(PushTargets::NoBranchTargets) | None => Vec::new(),
    }
}

fn blocked_branches(targets: &[String], protected_branches: &[String]) -> Vec<String> {
    if targets.is_empty() || protected_branches.is_empty() {
        return Vec::new();
    }
    if targets.iter().any(|target| target == ALL_BRANCHES_TARGET) {
        return unique_branches(protected_branches);
    }

    let mut blocked = Vec::new();
    for target in targets {
        if protected_branches.iter().any(|branch| branch == target)
            && !blocked.iter().any(|branch| branch == target)
        {
            blocked.push(target.clone());
        }
    }
    blocked
}

fn unique_branches(branches: &[String]) -> Vec<String> {
    let mut unique = Vec::new();
    for branch in branches {
        if !unique.iter().any(|existing| existing == branch) {
            unique.push(branch.clone());
        }
    }
    unique
}

fn format_blocked_push(blocked: &[String]) -> String {
    let branches = blocked
        .iter()
        .map(|branch| format!("'{branch}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Blocked: push to protected branch(es) {branches}. Protected branches can only be updated through pull requests."
    )
}

enum PushTargets<'a> {
    Refspecs(Vec<&'a str>),
    AllBranches,
    NoBranchTargets,
}

#[derive(Clone, Copy)]
struct SkipFlag {
    tokens: usize,
    repository_from_flag: bool,
}

impl SkipFlag {
    const fn new(tokens: usize, repository_from_flag: bool) -> Self {
        Self {
            tokens,
            repository_from_flag,
        }
    }
}

fn push_refspecs<'a>(tokens: &'a [&'a str]) -> Option<PushTargets<'a>> {
    if !is_git_push(tokens) {
        return None;
    }

    let mut positionals = Vec::new();
    let mut saw_tags = false;
    let mut repository_from_flag = false;
    let mut index = 2;

    while index < tokens.len() {
        let token = tokens[index];
        if token == "--delete" {
            return None;
        }
        if token == "--all" || token == "--mirror" {
            return Some(PushTargets::AllBranches);
        }
        if token == "--tags" {
            saw_tags = true;
            index += 1;
            continue;
        }
        if let Some(skip_flag) = should_skip_flag(token) {
            repository_from_flag |= skip_flag.repository_from_flag;
            index = skip_tokens(tokens, index, skip_flag.tokens)?;
            continue;
        }
        if token.starts_with('-') {
            return None;
        }
        positionals.push(token);
        index += 1;
    }

    classify_push_targets(positionals, saw_tags, repository_from_flag)
}

fn is_git_push(tokens: &[&str]) -> bool {
    tokens.len() >= 2 && tokens[0] == "git" && tokens[1] == "push"
}

fn skip_tokens(tokens: &[&str], index: usize, count: usize) -> Option<usize> {
    let next = index + count;
    (next <= tokens.len()).then_some(next)
}

fn classify_push_targets<'a>(
    positionals: Vec<&'a str>,
    saw_tags: bool,
    repository_from_flag: bool,
) -> Option<PushTargets<'a>> {
    if repository_from_flag {
        if !positionals.is_empty() {
            return Some(PushTargets::Refspecs(positionals));
        }
    } else if positionals.len() >= 2 {
        return Some(PushTargets::Refspecs(
            positionals.into_iter().skip(1).collect(),
        ));
    }

    if saw_tags {
        return Some(PushTargets::NoBranchTargets);
    }
    None
}

fn should_skip_flag(flag: &str) -> Option<SkipFlag> {
    if matches!(
        flag,
        "-f" | "--force"
            | "--no-verify"
            | "-u"
            | "--set-upstream"
            | "--force-with-lease"
            | "--quiet"
            | "-q"
            | "--verbose"
            | "-v"
            | "--dry-run"
            | "-n"
    ) || flag.starts_with("--force-with-lease=")
        || flag.starts_with("--push-option=")
        || flag.starts_with("--repo=")
        || flag.starts_with("--receive-pack=")
    {
        let repository_from_flag = flag.starts_with("--repo=");
        return Some(SkipFlag::new(1, repository_from_flag));
    }

    match flag {
        "-o" | "--push-option" | "--receive-pack" => Some(SkipFlag::new(2, false)),
        "--repo" => Some(SkipFlag::new(2, true)),
        _ => None,
    }
}

fn normalize_target(refspec: &str) -> Option<String> {
    let target = refspec_target(refspec)?;
    let target = target.strip_prefix("refs/heads/").unwrap_or(target);
    if target.is_empty() || target == "HEAD" || target.starts_with("refs/") {
        return None;
    }
    Some(target.to_string())
}

fn refspec_target(refspec: &str) -> Option<&str> {
    let cleaned = refspec.strip_prefix('+').unwrap_or(refspec);
    match cleaned.split_once(':') {
        Some((source, destination)) if source.is_empty() || destination.is_empty() => None,
        Some((_, destination)) => Some(destination),
        None => Some(cleaned),
    }
}

#[cfg(test)]
mod tests {
    use super::{check_push_allowed, extract_push_targets};

    #[test]
    fn check_push_allowed_blocks_protected_branches() {
        let targets = vec!["main".to_string(), "dev".to_string(), "staging".to_string()];
        let protected = vec!["main".to_string(), "staging".to_string()];

        let error = check_push_allowed(&targets, &protected).expect_err("push should be blocked");

        assert!(error.contains("'main'"));
        assert!(error.contains("'staging'"));
        assert!(error.contains("pull requests"));
    }

    #[test]
    fn check_push_allowed_allows_unprotected_branches() {
        let targets = vec!["dev".to_string(), "feature/ripcord".to_string()];
        let protected = vec!["main".to_string(), "staging".to_string()];

        assert!(check_push_allowed(&targets, &protected).is_ok());
    }

    #[test]
    fn check_push_allowed_allows_empty_protected_branches() {
        let targets = vec!["main".to_string()];
        let protected = Vec::new();

        assert!(check_push_allowed(&targets, &protected).is_ok());
    }

    #[test]
    fn check_push_allowed_allows_empty_targets() {
        let targets = Vec::new();
        let protected = vec!["main".to_string()];

        assert!(check_push_allowed(&targets, &protected).is_ok());
    }

    #[test]
    fn extract_push_targets_handles_supported_push_forms() {
        let cases = [
            ("git push origin main", vec!["main"]),
            ("git push origin main staging", vec!["main", "staging"]),
            ("git push origin HEAD:main", vec!["main"]),
            ("git push origin +main", vec!["main"]),
            ("git push origin refs/heads/main", vec!["main"]),
            ("git push -f origin main", vec!["main"]),
            ("git push --force origin main", vec!["main"]),
            ("git push --no-verify origin main", vec!["main"]),
            (
                "git push origin +HEAD:refs/heads/main refs/heads/staging",
                vec!["main", "staging"],
            ),
            ("git push -o ci.skip origin main", vec!["main"]),
            ("git push --push-option ci.skip origin main", vec!["main"]),
            ("git push --push-option=ci.skip origin main", vec!["main"]),
            (
                "git push --receive-pack git-receive-pack origin main",
                vec!["main"],
            ),
            (
                "git push --receive-pack=git-receive-pack origin main",
                vec!["main"],
            ),
            (
                "git push --repo ssh://example.com/repo.git main",
                vec!["main"],
            ),
            (
                "git push --repo=ssh://example.com/repo.git main",
                vec!["main"],
            ),
        ];

        for (command, expected) in cases {
            let actual = extract_push_targets(command);
            assert_eq!(actual, expected, "command: {command}");
        }
    }

    #[test]
    fn extract_push_targets_blocks_all_branches_push_when_protected_branches_exist() {
        let targets = extract_push_targets("git push --all origin");
        let protected = vec!["main".to_string(), "staging".to_string()];

        let error = check_push_allowed(&targets, &protected).expect_err("push should be blocked");

        assert!(error.contains("'main'"));
        assert!(error.contains("'staging'"));
    }

    #[test]
    fn extract_push_targets_blocks_mirror_push_when_protected_branches_exist() {
        let targets = extract_push_targets("git push --mirror origin");
        let protected = vec!["main".to_string(), "staging".to_string()];

        let error = check_push_allowed(&targets, &protected).expect_err("push should be blocked");

        assert!(error.contains("'main'"));
        assert!(error.contains("'staging'"));
    }

    #[test]
    fn extract_push_targets_allows_all_branches_push_when_no_branches_are_protected() {
        let targets = extract_push_targets("git push --all origin");
        let protected = Vec::new();

        assert!(check_push_allowed(&targets, &protected).is_ok());
    }

    #[test]
    fn extract_push_targets_ignores_tag_only_pushes() {
        assert!(extract_push_targets("git push --tags origin").is_empty());
    }

    #[test]
    fn extract_push_targets_returns_empty_when_target_cannot_be_determined() {
        let cases = [
            "git push",
            "git status",
            "git push --delete origin main",
            "git push origin :main",
            "git push --unknown origin main",
        ];

        for command in cases {
            assert!(
                extract_push_targets(command).is_empty(),
                "command: {command}"
            );
        }
    }
}

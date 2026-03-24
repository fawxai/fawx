/// Check whether any push targets are protected branches.
/// Returns Err with a user-facing message listing blocked branches.
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
    let Some(refspecs) = push_refspecs(&tokens) else {
        return Vec::new();
    };
    refspecs.into_iter().filter_map(normalize_target).collect()
}

fn blocked_branches(targets: &[String], protected_branches: &[String]) -> Vec<String> {
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

fn push_refspecs<'a>(tokens: &'a [&'a str]) -> Option<Vec<&'a str>> {
    if tokens.len() < 2 || tokens[0] != "git" || tokens[1] != "push" {
        return None;
    }
    let mut positionals = Vec::new();
    for token in &tokens[2..] {
        if *token == "--delete" {
            return None;
        }
        if should_skip_flag(token) {
            continue;
        }
        if token.starts_with('-') {
            return None;
        }
        positionals.push(*token);
    }
    if positionals.len() < 2 {
        return None;
    }
    Some(positionals.into_iter().skip(1).collect())
}

fn should_skip_flag(flag: &str) -> bool {
    matches!(
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
            | "--tags"
            | "--all"
            | "--mirror"
            | "--dry-run"
            | "-n"
    ) || flag.starts_with("--force-with-lease=")
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
        ];

        for (command, expected) in cases {
            let actual = extract_push_targets(command);
            assert_eq!(actual, expected, "command: {command}");
        }
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

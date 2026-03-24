const VERSION_LABEL: &str = "fawx";

pub fn run() -> i32 {
    for line in version_lines(
        env!("CARGO_PKG_VERSION"),
        option_env!("GIT_HASH"),
        option_env!("BUILD_DATE"),
        option_env!("TARGET_TRIPLE"),
        &compiled_features(),
    ) {
        println!("{line}");
    }
    0
}

fn version_lines(
    version: &str,
    git_hash: Option<&str>,
    build_date: Option<&str>,
    target_triple: Option<&str>,
    features: &[&str],
) -> Vec<String> {
    let mut lines = vec![format!("{VERSION_LABEL} {version}")];
    lines.push(format!("commit: {}", git_hash.unwrap_or("unknown")));
    lines.push(format!("built:  {}", build_date.unwrap_or("unknown")));
    lines.push(format!("built for: {}", target_triple.unwrap_or("unknown")));
    if !features.is_empty() {
        lines.push(format!("features: {}", features.join(", ")));
    }
    lines
}

fn compiled_features() -> Vec<&'static str> {
    [
        cfg!(feature = "http").then_some("http"),
        cfg!(feature = "oauth-bridge").then_some("oauth-bridge"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_output_includes_version_number() {
        let lines = version_lines(
            "1.2.3",
            Some("abc123"),
            Some("2026-03-10"),
            Some("x86_64-unknown-linux-gnu"),
            &["http"],
        );
        assert_eq!(lines[0], "fawx 1.2.3");
        assert!(lines
            .iter()
            .any(|line| line == "built for: x86_64-unknown-linux-gnu"));
        assert!(lines.iter().any(|line| line == "features: http"));
    }

    #[test]
    fn version_handles_missing_git_hash_gracefully() {
        let lines = version_lines("1.2.3", None, Some("2026-03-10"), None, &[]);
        assert!(lines.iter().any(|line| line == "commit: unknown"));
        assert!(lines.iter().any(|line| line == "built for: unknown"));
    }
}

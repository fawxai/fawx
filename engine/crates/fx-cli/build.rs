use chrono::Utc;
use std::process::Command;

fn main() {
    emit_git_hash();
    emit_build_date();
    emit_target_triple();
}

fn emit_git_hash() {
    if let Some(hash) = command_output("git", &["rev-parse", "--short", "HEAD"]) {
        println!("cargo:rustc-env=GIT_HASH={hash}");
    }
}

fn emit_build_date() {
    let date = Utc::now().format("%Y-%m-%d");
    println!("cargo:rustc-env=BUILD_DATE={date}");
}

fn emit_target_triple() {
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=TARGET_TRIPLE={target}");
    }
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

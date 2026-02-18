use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates dir")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn android_hello_source_has_module_docs_and_expected_message() {
    let path = repo_root().join("crates/ct-cli/src/bin/android_hello.rs");
    let source = std::fs::read_to_string(path).expect("read android_hello.rs");

    assert!(
        source.starts_with("//!"),
        "android_hello.rs should begin with module-level docs"
    );
    assert!(
        source.contains("hello from citros android (aarch64-linux-android)"),
        "android_hello.rs should emit the expected validation message"
    );
}

#[test]
fn build_android_hello_script_has_help_output() {
    let script = repo_root().join("scripts/build-android-hello.sh");

    let output = Command::new("bash")
        .arg(script)
        .arg("--help")
        .output()
        .expect("run build-android-hello.sh --help");

    assert!(output.status.success(), "help command should succeed");

    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("ANDROID_NDK_HOME"));
    assert!(stdout.contains("ANDROID_API"));
}

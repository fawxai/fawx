use serde::Deserialize;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::{Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
enum TestError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    TomlSerialize(toml::ser::Error),
    Validation(String),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::Io(e) => write!(f, "IO error: {e}"),
            TestError::Toml(e) => write!(f, "TOML parse error: {e}"),
            TestError::TomlSerialize(e) => write!(f, "TOML serialize error: {e}"),
            TestError::Validation(msg) => write!(f, "Validation error: {msg}"),
        }
    }
}

impl From<std::io::Error> for TestError {
    fn from(e: std::io::Error) -> Self {
        TestError::Io(e)
    }
}

impl From<toml::de::Error> for TestError {
    fn from(e: toml::de::Error) -> Self {
        TestError::Toml(e)
    }
}

impl From<toml::ser::Error> for TestError {
    fn from(e: toml::ser::Error) -> Self {
        TestError::TomlSerialize(e)
    }
}

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ScenarioFile {
    scenario: ScenarioMeta,
    #[serde(default)]
    setup: SetupConfig,
    input: InputConfig,
    expect: Expectations,
}

#[derive(Debug, Deserialize)]
struct ScenarioMeta {
    name: String,
    #[serde(default)]
    #[allow(dead_code)] // Deserialized from TOML, used for documentation
    description: String,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    30
}

#[derive(Debug, Default, Deserialize)]
struct SetupConfig {
    #[serde(default)]
    files: Vec<SetupFile>,
}

#[derive(Debug, Deserialize)]
struct SetupFile {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct InputConfig {
    prompt: String,
}

#[derive(Debug, Default, Deserialize)]
struct Expectations {
    #[serde(default)]
    tool_calls: Option<Vec<String>>,
    #[serde(default)]
    tool_input_contains: Option<Vec<String>>,
    #[serde(default)]
    output_contains: Option<Vec<String>>,
    #[serde(default)]
    output_not_contains: Option<Vec<String>>,
    #[serde(default)]
    no_tool_errors: Option<bool>,
}

/// Parsed output from a `fawx serve --single --json` run.
#[derive(Debug, Default)]
struct FawxOutput {
    tool_calls: Vec<String>,
    tool_inputs: Vec<String>,
    response_text: String,
    tool_errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct HeadlessJsonOutput {
    response: String,
    #[serde(default)]
    tool_calls: Vec<String>,
    #[serde(default)]
    tool_inputs: Vec<String>,
    #[serde(default)]
    tool_errors: Vec<String>,
}

struct ScenarioRuntime {
    work_dir: TempDir,
    data_dir: TempDir,
}

/// Result of running a single scenario.
struct ScenarioResult {
    name: String,
    passed: bool,
    failures: Vec<String>,
    duration_ms: u64,
}

/// CLI arguments parsed from `std::env::args`.
struct CliArgs {
    path: PathBuf,
    filter: Option<String>,
    timeout_override: Option<u64>,
}

// ── Scenario parsing ────────────────────────────────────────────────────────

fn parse_scenario(path: &Path) -> Result<ScenarioFile, TestError> {
    let content = std::fs::read_to_string(path)?;
    let scenario: ScenarioFile = toml::from_str(&content)?;
    validate_scenario(&scenario)?;
    Ok(scenario)
}

fn validate_scenario(scenario: &ScenarioFile) -> Result<(), TestError> {
    if scenario.input.prompt.trim().is_empty() {
        return Err(TestError::Validation(
            "scenario input.prompt must not be empty".to_string(),
        ));
    }
    Ok(())
}

// ── Scenario discovery ──────────────────────────────────────────────────────

fn discover_scenarios(path: &Path, filter: Option<&str>) -> Result<Vec<PathBuf>, TestError> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    let mut results = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.extension() == Some(OsStr::new("toml")) {
            if let Some(pattern) = filter {
                let name = entry_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if !name.contains(pattern) {
                    continue;
                }
            }
            results.push(entry_path);
        }
    }
    results.sort();
    Ok(results)
}

// ── Temp dir setup ──────────────────────────────────────────────────────────

fn setup_temp_dir(setup: &SetupConfig) -> Result<TempDir, TestError> {
    let tmp = TempDir::new()?;
    for file in &setup.files {
        let file_path = tmp.path().join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::create(&file_path)?;
        f.write_all(file.content.as_bytes())?;
    }
    Ok(tmp)
}

const SCENARIO_DATA_FILES: &[&str] = &[
    "config.toml",
    "auth.db",
    ".auth-salt",
    "credentials.db",
    ".credentials-salt",
];

fn prepare_scenario_runtime(setup: &SetupConfig) -> Result<ScenarioRuntime, TestError> {
    let work_dir = setup_temp_dir(setup)?;
    let data_dir = prepare_data_dir(work_dir.path())?;
    Ok(ScenarioRuntime { work_dir, data_dir })
}

fn prepare_data_dir(work_dir: &Path) -> Result<TempDir, TestError> {
    let data_dir = TempDir::new()?;
    copy_runtime_state(&source_data_dir()?, data_dir.path())?;
    patch_runtime_config(data_dir.path(), work_dir)?;
    Ok(data_dir)
}

fn source_data_dir() -> Result<PathBuf, TestError> {
    if let Ok(path) = std::env::var("FAWX_TEST_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var("HOME")
        .map_err(|_| TestError::Validation("HOME not set; set FAWX_TEST_DATA_DIR".to_string()))?;
    Ok(PathBuf::from(home).join(".fawx"))
}

fn copy_runtime_state(source: &Path, dest: &Path) -> Result<(), TestError> {
    for name in SCENARIO_DATA_FILES {
        let src = source.join(name);
        if src.exists() {
            std::fs::copy(&src, dest.join(name))?;
        }
    }
    Ok(())
}

fn patch_runtime_config(data_dir: &Path, work_dir: &Path) -> Result<(), TestError> {
    let config_path = data_dir.join("config.toml");
    let config_text = std::fs::read_to_string(&config_path)?;
    let mut config: toml::Value = toml::from_str(&config_text)?;
    let table = config
        .as_table_mut()
        .ok_or_else(|| TestError::Validation("config.toml must be a table".to_string()))?;
    let tools = table
        .entry("tools")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| TestError::Validation("[tools] must be a table".to_string()))?;
    tools.insert(
        "working_dir".to_string(),
        toml::Value::String(work_dir.display().to_string()),
    );
    std::fs::write(config_path, toml::to_string(&config)?)?;
    Ok(())
}

// ── Fawx subprocess ─────────────────────────────────────────────────────────

fn find_fawx_binary() -> Result<PathBuf, TestError> {
    // Check FAWX_BIN env var first, then PATH
    if let Ok(bin) = std::env::var("FAWX_BIN") {
        let path = PathBuf::from(bin);
        if path.exists() {
            return Ok(path);
        }
    }

    // Try to find in PATH via `which`
    let output = Command::new("which").arg("fawx").output();
    if let Ok(out) = output {
        if out.status.success() {
            let path_str = String::from_utf8_lossy(&out.stdout);
            let path = PathBuf::from(path_str.trim());
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Err(TestError::Validation(
        "fawx binary not found. Set FAWX_BIN or add fawx to PATH".to_string(),
    ))
}

fn spawn_fawx(
    bin: &Path,
    prompt: &str,
    work_dir: &Path,
    data_dir: &Path,
    timeout: u64,
) -> Result<FawxOutput, TestError> {
    let mut child = Command::new(bin)
        .args([
            "serve",
            "--single",
            "--json",
            "--data-dir",
            data_dir.to_str().ok_or_else(|| {
                TestError::Validation("data dir path must be valid UTF-8".to_string())
            })?,
        ])
        .current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            TestError::Io(std::io::Error::new(
                e.kind(),
                format!("failed to spawn fawx: {e}"),
            ))
        })?;
    write_json_input(&mut child, prompt)?;

    let status = wait_with_timeout(&mut child, timeout)?;

    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }

    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_string(&mut stderr);
        }
        return Err(TestError::Validation(format!(
            "fawx exited with {status}: {stderr}"
        )));
    }

    parse_fawx_output(&stdout)
}

fn write_json_input(child: &mut std::process::Child, prompt: &str) -> Result<(), TestError> {
    let payload = format!("{}\n", serde_json::json!({ "message": prompt }));
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| TestError::Validation("fawx stdin unavailable".to_string()))?;
    stdin.write_all(payload.as_bytes())?;
    Ok(())
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: u64,
) -> Result<std::process::ExitStatus, TestError> {
    let deadline = Duration::from_secs(timeout);
    let start = Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(status),
            None => {
                if start.elapsed() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TestError::Validation(format!(
                        "fawx timed out after {timeout}s"
                    )));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn parse_fawx_output(raw: &str) -> Result<FawxOutput, TestError> {
    let line = raw
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| TestError::Validation("fawx emitted no JSON output".to_string()))?;
    let output: HeadlessJsonOutput = serde_json::from_str(line).map_err(|e| {
        TestError::Validation(format!(
            "failed to parse fawx JSON output: {e}; raw={raw:?}"
        ))
    })?;
    Ok(FawxOutput {
        tool_calls: output.tool_calls,
        tool_inputs: output.tool_inputs,
        response_text: output.response,
        tool_errors: output.tool_errors,
    })
}

// ── Expectation checking ────────────────────────────────────────────────────

fn check_expectations(output: &FawxOutput, expect: &Expectations) -> Vec<String> {
    let mut failures = Vec::new();

    check_tool_calls(output, expect, &mut failures);
    check_tool_input_contains(output, expect, &mut failures);
    check_output_contains(output, expect, &mut failures);
    check_output_not_contains(output, expect, &mut failures);
    check_no_tool_errors(output, expect, &mut failures);

    failures
}

fn check_tool_calls(output: &FawxOutput, expect: &Expectations, failures: &mut Vec<String>) {
    if let Some(expected_tools) = &expect.tool_calls {
        let actual: HashSet<&str> = output.tool_calls.iter().map(|s| s.as_str()).collect();
        for tool in expected_tools {
            if !actual.contains(tool.as_str()) {
                failures.push(format!(
                    "Expected tool_calls to include \"{tool}\", got: {:?}",
                    output.tool_calls
                ));
            }
        }
    }
}

fn check_tool_input_contains(
    output: &FawxOutput,
    expect: &Expectations,
    failures: &mut Vec<String>,
) {
    if let Some(patterns) = &expect.tool_input_contains {
        let joined_inputs = output.tool_inputs.join("\n").to_lowercase();
        for pattern in patterns {
            if !joined_inputs.contains(&pattern.to_lowercase()) {
                failures.push(format!(
                    "Expected tool inputs to contain \"{pattern}\", not found"
                ));
            }
        }
    }
}

fn check_output_contains(output: &FawxOutput, expect: &Expectations, failures: &mut Vec<String>) {
    if let Some(patterns) = &expect.output_contains {
        let lower = output.response_text.to_lowercase();
        for pattern in patterns {
            if !lower.contains(&pattern.to_lowercase()) {
                failures.push(format!(
                    "Expected output to contain \"{pattern}\", not found"
                ));
            }
        }
    }
}

fn check_output_not_contains(
    output: &FawxOutput,
    expect: &Expectations,
    failures: &mut Vec<String>,
) {
    if let Some(patterns) = &expect.output_not_contains {
        let lower = output.response_text.to_lowercase();
        for pattern in patterns {
            if lower.contains(&pattern.to_lowercase()) {
                failures.push(format!(
                    "Expected output NOT to contain \"{pattern}\", but found it"
                ));
            }
        }
    }
}

fn check_no_tool_errors(output: &FawxOutput, expect: &Expectations, failures: &mut Vec<String>) {
    if expect.no_tool_errors == Some(true) && !output.tool_errors.is_empty() {
        failures.push(format!(
            "Expected no tool errors, got: {:?}",
            output.tool_errors
        ));
    }
}

// ── Running scenarios ───────────────────────────────────────────────────────

fn run_scenario(scenario: &ScenarioFile, fawx_bin: &Path) -> ScenarioResult {
    let start = Instant::now();
    let name = scenario.scenario.name.clone();

    let result = run_scenario_inner(scenario, fawx_bin);

    let duration_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(failures) => ScenarioResult {
            name,
            passed: failures.is_empty(),
            failures,
            duration_ms,
        },
        Err(e) => ScenarioResult {
            name,
            passed: false,
            failures: vec![format!("Setup/execution error: {e}")],
            duration_ms,
        },
    }
}

fn run_scenario_inner(scenario: &ScenarioFile, fawx_bin: &Path) -> Result<Vec<String>, TestError> {
    let runtime = prepare_scenario_runtime(&scenario.setup)?;
    let output = spawn_fawx(
        fawx_bin,
        &scenario.input.prompt,
        runtime.work_dir.path(),
        runtime.data_dir.path(),
        scenario.scenario.timeout_seconds,
    )?;
    Ok(check_expectations(&output, &scenario.expect))
}

// ── Output formatting ───────────────────────────────────────────────────────

fn print_summary(results: &[ScenarioResult]) {
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;
    let total_ms: u64 = results.iter().map(|r| r.duration_ms).sum();

    println!();
    for result in results {
        if result.passed {
            println!(
                "  \u{2713} {} ({:.1}s)",
                result.name,
                result.duration_ms as f64 / 1000.0
            );
        } else {
            println!(
                "  \u{2717} {} ({:.1}s)",
                result.name,
                result.duration_ms as f64 / 1000.0
            );
            for failure in &result.failures {
                println!("    - {failure}");
            }
        }
    }

    println!(
        "\nResults: {passed} passed, {failed} failed ({:.1}s total)",
        total_ms as f64 / 1000.0
    );
}

// ── CLI argument parsing ────────────────────────────────────────────────────

fn parse_args() -> Result<CliArgs, TestError> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        return Err(TestError::Validation(
            "Usage: fawx-test <path> [--filter <name>] [--timeout <seconds>]".to_string(),
        ));
    }

    let path = PathBuf::from(&args[1]);
    let mut filter = None;
    let mut timeout_override = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" => {
                i += 1;
                filter = args.get(i).cloned();
            }
            "--timeout" => {
                i += 1;
                timeout_override = args.get(i).and_then(|s| s.parse::<u64>().ok());
            }
            _ => {}
        }
        i += 1;
    }

    Ok(CliArgs {
        path,
        filter,
        timeout_override,
    })
}

// ── Scenario batch runner ────────────────────────────────────────────────────

fn run_scenarios(
    scenarios: &[PathBuf],
    fawx_bin: &Path,
    timeout_override: Option<u64>,
) -> Vec<ScenarioResult> {
    let mut results = Vec::new();
    for path in scenarios {
        let scenario = match parse_scenario(path) {
            Ok(s) => s,
            Err(e) => {
                results.push(ScenarioResult {
                    name: path.display().to_string(),
                    passed: false,
                    failures: vec![format!("Parse error: {e}")],
                    duration_ms: 0,
                });
                continue;
            }
        };

        let mut scenario_to_run = scenario;
        if let Some(t) = timeout_override {
            scenario_to_run.scenario.timeout_seconds = t;
        }

        results.push(run_scenario(&scenario_to_run, fawx_bin));
    }
    results
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    println!("fawx-test v0.1.0 — behavioral test harness\n");

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(2);
        }
    };

    let fawx_bin = match find_fawx_binary() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(2);
        }
    };

    let scenarios = match discover_scenarios(&args.path, args.filter.as_deref()) {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => {
            eprintln!("No scenarios found.");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("Error discovering scenarios: {e}");
            std::process::exit(2);
        }
    };

    println!("Running {} scenarios...", scenarios.len());
    let results = run_scenarios(&scenarios, &fawx_bin, args.timeout_override);
    print_summary(&results);

    let any_failed = results.iter().any(|r| !r.passed);
    std::process::exit(if any_failed { 1 } else { 0 });
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_valid_scenario() {
        let toml_content = r#"
[scenario]
name = "test_basic"
description = "A basic test"
timeout_seconds = 10

[setup]
files = []

[input]
prompt = "Hello world"

[expect]
output_contains = ["hello"]
"#;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.toml");
        fs::write(&path, toml_content).unwrap();

        let scenario = parse_scenario(&path).unwrap();
        assert_eq!(scenario.scenario.name, "test_basic");
        assert_eq!(scenario.input.prompt, "Hello world");
        assert_eq!(scenario.scenario.timeout_seconds, 10);
    }

    #[test]
    fn parse_invalid_scenario_missing_prompt() {
        let toml_content = r#"
[scenario]
name = "bad_scenario"

[input]
prompt = ""

[expect]
"#;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, toml_content).unwrap();

        let result = parse_scenario(&path);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("prompt"));
    }

    #[test]
    fn discover_finds_toml_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.toml"), "").unwrap();
        fs::write(dir.path().join("b.toml"), "").unwrap();
        fs::write(dir.path().join("c.txt"), "").unwrap();

        let found = discover_scenarios(dir.path(), None).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| p.extension().unwrap() == "toml"));
    }

    #[test]
    fn discover_filters_by_name() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("memory-write.toml"), "").unwrap();
        fs::write(dir.path().join("file-read.toml"), "").unwrap();
        fs::write(dir.path().join("basic-response.toml"), "").unwrap();

        let found = discover_scenarios(dir.path(), Some("memory")).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0]
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("memory"));
    }

    #[test]
    fn check_expectations_passes_on_match() {
        let output = FawxOutput {
            tool_calls: vec!["read_file".to_string()],
            tool_inputs: vec![r#"{"path":"README.md"}"#.to_string()],
            response_text: "The file contains hello world".to_string(),
            tool_errors: vec![],
        };

        let expect = Expectations {
            tool_calls: Some(vec!["read_file".to_string()]),
            tool_input_contains: Some(vec!["readme".to_string()]),
            output_contains: Some(vec!["hello world".to_string()]),
            output_not_contains: Some(vec!["error".to_string()]),
            no_tool_errors: Some(true),
        };

        let failures = check_expectations(&output, &expect);
        assert!(
            failures.is_empty(),
            "Expected no failures, got: {failures:?}"
        );
    }

    #[test]
    fn check_expectations_fails_on_mismatch() {
        let output = FawxOutput {
            tool_calls: vec!["read_file".to_string()],
            tool_inputs: vec![r#"{"path":"README.md"}"#.to_string()],
            response_text: "Something went wrong".to_string(),
            tool_errors: vec!["file not found".to_string()],
        };

        let expect = Expectations {
            tool_calls: Some(vec!["memory_write".to_string()]),
            tool_input_contains: Some(vec!["blue".to_string()]),
            output_contains: Some(vec!["hello".to_string()]),
            output_not_contains: Some(vec!["wrong".to_string()]),
            no_tool_errors: Some(true),
        };

        let failures = check_expectations(&output, &expect);
        assert_eq!(failures.len(), 5);
    }

    #[test]
    fn setup_temp_dir_creates_files() {
        let setup = SetupConfig {
            files: vec![
                SetupFile {
                    path: "test.txt".to_string(),
                    content: "hello world".to_string(),
                },
                SetupFile {
                    path: "subdir/nested.txt".to_string(),
                    content: "nested content".to_string(),
                },
            ],
        };

        let dir = setup_temp_dir(&setup).unwrap();

        let content = fs::read_to_string(dir.path().join("test.txt")).unwrap();
        assert_eq!(content, "hello world");

        let nested = fs::read_to_string(dir.path().join("subdir/nested.txt")).unwrap();
        assert_eq!(nested, "nested content");
    }

    #[test]
    fn output_contains_is_case_insensitive() {
        let output = FawxOutput {
            tool_calls: vec![],
            tool_inputs: vec![],
            response_text: "Hello World".to_string(),
            tool_errors: vec![],
        };

        let expect = Expectations {
            tool_calls: None,
            tool_input_contains: None,
            output_contains: Some(vec!["hello world".to_string()]),
            output_not_contains: None,
            no_tool_errors: None,
        };

        let failures = check_expectations(&output, &expect);
        assert!(
            failures.is_empty(),
            "Case-insensitive match should pass, got: {failures:?}"
        );

        // Also check that uppercase pattern matches lowercase output
        let output2 = FawxOutput {
            tool_calls: vec![],
            tool_inputs: vec![],
            response_text: "hello world".to_string(),
            tool_errors: vec![],
        };

        let expect2 = Expectations {
            tool_calls: None,
            tool_input_contains: None,
            output_contains: Some(vec!["Hello World".to_string()]),
            output_not_contains: None,
            no_tool_errors: None,
        };

        let failures2 = check_expectations(&output2, &expect2);
        assert!(
            failures2.is_empty(),
            "Case-insensitive match should pass (reverse), got: {failures2:?}"
        );
    }

    #[test]
    fn parse_fawx_output_reads_headless_json_envelope() {
        let raw = r#"{"response":"hello","tool_calls":["read_file"],"tool_errors":["missing"]}"#;

        let output = parse_fawx_output(raw).unwrap();

        assert_eq!(output.response_text, "hello");
        assert_eq!(output.tool_calls, vec!["read_file"]);
        assert!(output.tool_inputs.is_empty());
        assert_eq!(output.tool_errors, vec!["missing"]);
    }

    #[test]
    fn spawn_fawx_writes_json_input_to_stdin() {
        let dir = TempDir::new().unwrap();
        let capture_path = dir.path().join("input.json");
        let script_path = dir.path().join("mock-fawx.sh");
        let script = format!(
            "#!/bin/sh\ncat > \"{}\"\nprintf '%s\\n' '{}' \n",
            capture_path.display(),
            r#"{"response":"ok","tool_calls":["read_file"],"tool_errors":[]}"#
        );
        fs::write(&script_path, script).unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).unwrap();
        }

        let output = spawn_fawx(&script_path, "hello world", dir.path(), dir.path(), 5).unwrap();
        let captured = fs::read_to_string(capture_path).unwrap();

        assert_eq!(output.tool_calls, vec!["read_file"]);
        assert!(output.tool_inputs.is_empty());
        assert_eq!(captured.trim(), r#"{"message":"hello world"}"#);
    }
}

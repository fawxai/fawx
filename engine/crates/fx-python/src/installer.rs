use crate::process::{
    elapsed_millis, format_process_detail, run_command, CapturedProcess, ProcessStatus,
    MAX_TIMEOUT_SECONDS,
};
use crate::venv::{PackageInfo, VenvManager};
use fx_kernel::cancellation::CancellationToken;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::process::Command;

const DEFAULT_INSTALL_TIMEOUT_SECONDS: u64 = 600;

#[derive(Debug, Clone)]
pub struct PythonInstaller {
    manager: VenvManager,
    experiments_root: PathBuf,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PythonInstallArgs {
    #[serde(default)]
    pub packages: Vec<String>,
    pub venv: String,
    #[serde(default)]
    pub requirements_file: Option<String>,
    #[serde(default = "default_install_timeout_seconds")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct InstallResult {
    pub installed: Vec<String>,
    pub duration_ms: u64,
}

struct InstallPlan {
    requirements_file: Option<PathBuf>,
    timeout_seconds: u64,
}

impl PythonInstaller {
    #[must_use]
    pub fn new(manager: VenvManager, experiments_root: PathBuf) -> Self {
        Self {
            manager,
            experiments_root,
        }
    }

    pub async fn install(
        &self,
        args: PythonInstallArgs,
        cancel: Option<&CancellationToken>,
    ) -> Result<InstallResult, String> {
        validate_install_request(&args)?;
        self.manager.ensure_venv(&args.venv).await?;
        let plan = self.plan_install(&args).await?;

        let started = Instant::now();
        let output = self.run_pip_install(&args, &plan, cancel).await?;
        let installed = self
            .resolve_installed_packages(&args, &output.stdout)
            .await?;

        Ok(InstallResult {
            installed,
            duration_ms: elapsed_millis(started.elapsed()),
        })
    }

    async fn plan_install(&self, args: &PythonInstallArgs) -> Result<InstallPlan, String> {
        Ok(InstallPlan {
            requirements_file: self.resolve_requirements_file(args).await?,
            timeout_seconds: clamp_timeout_seconds(args.timeout_seconds),
        })
    }

    async fn run_pip_install(
        &self,
        args: &PythonInstallArgs,
        plan: &InstallPlan,
        cancel: Option<&CancellationToken>,
    ) -> Result<CapturedProcess, String> {
        let mut command = Command::new(self.manager.pip_path(&args.venv));
        configure_install_command(&mut command, args, plan);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = run_command(
            command,
            "pip install",
            Duration::from_secs(plan.timeout_seconds),
            cancel,
        )
        .await?;
        require_success(output, plan.timeout_seconds)
    }

    async fn resolve_installed_packages(
        &self,
        args: &PythonInstallArgs,
        stdout: &str,
    ) -> Result<Vec<String>, String> {
        let parsed = parse_pip_output(stdout);
        if !parsed.is_empty() || args.requirements_file.is_some() {
            return Ok(parsed);
        }

        let installed = self.manager.info(&args.venv).await?;
        Ok(match_requested_packages(&installed, &args.packages))
    }

    async fn resolve_requirements_file(
        &self,
        args: &PythonInstallArgs,
    ) -> Result<Option<PathBuf>, String> {
        let Some(requirements_file) = &args.requirements_file else {
            return Ok(None);
        };

        let experiment_dir = self.experiments_root.join(&args.venv);
        let base = canonicalize_dir(&experiment_dir).await?;
        let candidate = requirements_path(&base, requirements_file);
        let path = canonicalize_file(&candidate).await?;
        ensure_path_within(&base, &path)?;
        Ok(Some(path))
    }
}

fn validate_install_request(args: &PythonInstallArgs) -> Result<(), String> {
    if args.packages.is_empty() && args.requirements_file.is_none() {
        return Err("python_install requires 'packages' or 'requirements_file'".to_string());
    }

    Ok(())
}

fn configure_install_command(command: &mut Command, args: &PythonInstallArgs, plan: &InstallPlan) {
    command.arg("install").arg("--no-cache-dir");
    if let Some(requirements_file) = &plan.requirements_file {
        command.arg("-r").arg(requirements_file);
        return;
    }

    command.args(args.packages.iter().map(String::as_str));
}

fn require_success(
    output: CapturedProcess,
    timeout_seconds: u64,
) -> Result<CapturedProcess, String> {
    match output.status {
        ProcessStatus::Exited(0) => Ok(output),
        ProcessStatus::Exited(exit_code) => Err(format!(
            "pip install failed: {}",
            format_process_detail(Some(exit_code), &output.stdout, &output.stderr)
        )),
        ProcessStatus::TimedOut => Err(timeout_message(&output, timeout_seconds)),
        ProcessStatus::Cancelled => Err("pip install cancelled".to_string()),
    }
}

fn timeout_message(output: &CapturedProcess, timeout_seconds: u64) -> String {
    let detail = format_process_detail(None, &output.stdout, &output.stderr);
    format!("pip install timed out after {timeout_seconds} seconds: {detail}")
}

async fn canonicalize_dir(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).await.map_err(|error| {
        format!(
            "experiment dir '{}' is unavailable: {error}",
            path.display()
        )
    })
}

async fn canonicalize_file(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).await.map_err(|error| {
        format!(
            "requirements file '{}' is unavailable: {error}",
            path.display()
        )
    })?;
    let metadata = fs::metadata(&canonical)
        .await
        .map_err(|error| format!("failed to inspect '{}': {error}", canonical.display()))?;
    if metadata.is_file() {
        return Ok(canonical);
    }

    Err(format!(
        "requirements file '{}' must be a file",
        canonical.display()
    ))
}

fn requirements_path(base: &Path, requirements_file: &str) -> PathBuf {
    let path = Path::new(requirements_file);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base.join(path)
}

fn ensure_path_within(base: &Path, path: &Path) -> Result<(), String> {
    if path.starts_with(base) {
        return Ok(());
    }

    Err(format!(
        "requirements file '{}' must stay inside experiment dir '{}'",
        path.display(),
        base.display()
    ))
}

pub(crate) fn parse_pip_output(stdout: &str) -> Vec<String> {
    let mut installed = Vec::new();
    for line in stdout.lines() {
        if let Some(packages) = line.trim().strip_prefix("Successfully installed ") {
            for token in packages.split_whitespace() {
                if let Some(package) = parse_installed_token(token) {
                    installed.push(package);
                }
            }
        }
    }
    installed
}

fn parse_installed_token(token: &str) -> Option<String> {
    let cleaned = token.trim_matches(|ch: char| ch == ',' || ch == ';');
    let (name, version) = cleaned.rsplit_once('-')?;
    if name.is_empty() || version.is_empty() {
        return None;
    }

    Some(format!("{name}=={version}"))
}

fn match_requested_packages(installed: &[PackageInfo], requested: &[String]) -> Vec<String> {
    let mut matches = Vec::new();
    for spec in requested {
        if let Some(name) = requested_name(spec) {
            if let Some(package) = find_package(installed, &name) {
                matches.push(format!("{}=={}", package.name, package.version));
            }
        }
    }
    matches
}

fn find_package<'a>(installed: &'a [PackageInfo], name: &str) -> Option<&'a PackageInfo> {
    installed
        .iter()
        .find(|package| normalize_name(&package.name) == name)
}

fn requested_name(spec: &str) -> Option<String> {
    let name: String = spec
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    if name.is_empty() {
        return None;
    }

    Some(normalize_name(&name))
}

fn normalize_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

fn clamp_timeout_seconds(timeout_seconds: u64) -> u64 {
    timeout_seconds.min(MAX_TIMEOUT_SECONDS)
}

fn default_install_timeout_seconds() -> u64 {
    DEFAULT_INSTALL_TIMEOUT_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn parse_pip_output_extracts_installed_packages() {
        let stdout = "Collecting requests\nSuccessfully installed requests-2.32.3 urllib3-2.2.2 my-package-1.0.0\n";

        let installed = parse_pip_output(stdout);

        assert_eq!(
            installed,
            vec![
                "requests==2.32.3".to_string(),
                "urllib3==2.2.2".to_string(),
                "my-package==1.0.0".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn install_rejects_requirements_file_outside_experiment_dir() {
        let temp_dir = TempDir::new().expect("tempdir");
        let installer = installer_with_fake_pip(&temp_dir, success_pip_script("unused"));
        let experiment_dir = temp_dir.path().join("experiments/test");
        fs::create_dir_all(&experiment_dir).expect("experiment dir");
        let outside = temp_dir.path().join("outside.txt");
        fs::write(&outside, "requests==2.32.3\n").expect("outside requirements");

        let error = installer
            .install(
                PythonInstallArgs {
                    packages: Vec::new(),
                    venv: "test".to_string(),
                    requirements_file: Some(outside.to_string_lossy().into_owned()),
                    timeout_seconds: 600,
                },
                None,
            )
            .await
            .expect_err("outside requirements should fail");

        assert!(error.contains("must stay inside experiment dir"));
    }

    #[tokio::test]
    async fn install_adds_no_cache_dir_and_sandboxed_requirements_file() {
        let temp_dir = TempDir::new().expect("tempdir");
        let args_file = temp_dir.path().join("pip-args.txt");
        let installer = installer_with_fake_pip(
            &temp_dir,
            success_pip_script(args_file.to_string_lossy().as_ref()),
        );
        let experiment_dir = temp_dir.path().join("experiments/test");
        fs::create_dir_all(&experiment_dir).expect("experiment dir");
        fs::write(
            experiment_dir.join("requirements.txt"),
            "requests==2.32.3\n",
        )
        .expect("requirements file");

        installer
            .install(
                PythonInstallArgs {
                    packages: Vec::new(),
                    venv: "test".to_string(),
                    requirements_file: Some("requirements.txt".to_string()),
                    timeout_seconds: 600,
                },
                None,
            )
            .await
            .expect("install should succeed");

        let args = fs::read_to_string(&args_file).expect("pip args");
        let lines: Vec<_> = args.lines().collect();
        let canonical_experiment_dir =
            std::fs::canonicalize(&experiment_dir).expect("canonicalize experiment dir");
        assert_eq!(lines[0], "install");
        assert_eq!(lines[1], "--no-cache-dir");
        assert_eq!(lines[2], "-r");
        assert!(lines[3].starts_with(canonical_experiment_dir.to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn install_times_out() {
        let temp_dir = TempDir::new().expect("tempdir");
        let installer = installer_with_fake_pip(&temp_dir, sleeping_pip_script());

        let error = installer
            .install(
                PythonInstallArgs {
                    packages: vec!["demo".to_string()],
                    venv: "test".to_string(),
                    requirements_file: None,
                    timeout_seconds: 1,
                },
                None,
            )
            .await
            .expect_err("sleeping pip should time out");

        assert!(error.contains("timed out"));
    }

    #[tokio::test]
    async fn cancellation_stops_pip_install() {
        let temp_dir = TempDir::new().expect("tempdir");
        let installer = installer_with_fake_pip(&temp_dir, sleeping_pip_script());

        let token = CancellationToken::new();
        let cancel = token.clone();

        // Cancel after 200ms — well before the sleeping pip would finish
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel.cancel();
        });

        let error = installer
            .install(
                PythonInstallArgs {
                    packages: vec!["demo".to_string()],
                    venv: "test".to_string(),
                    requirements_file: None,
                    timeout_seconds: 600,
                },
                Some(&token),
            )
            .await
            .expect_err("cancelled pip should fail");

        assert!(
            error.contains("cancelled"),
            "expected cancelled, got: {error}"
        );
    }

    #[tokio::test]
    async fn install_adds_no_cache_dir_for_package_installs() {
        let temp_dir = TempDir::new().expect("tempdir");
        let args_file = temp_dir.path().join("package-args.txt");
        let installer = installer_with_fake_pip(
            &temp_dir,
            success_pip_script(args_file.to_string_lossy().as_ref()),
        );

        installer
            .install(
                PythonInstallArgs {
                    packages: vec!["demo".to_string()],
                    venv: "test".to_string(),
                    requirements_file: None,
                    timeout_seconds: 600,
                },
                None,
            )
            .await
            .expect("install should succeed");

        let args = fs::read_to_string(&args_file).expect("pip args");
        let lines: Vec<_> = args.lines().collect();
        assert_eq!(lines[0], "install");
        assert_eq!(lines[1], "--no-cache-dir");
        assert_eq!(lines[2], "demo");
    }

    #[test]
    fn timeout_seconds_are_clamped() {
        assert_eq!(
            clamp_timeout_seconds(MAX_TIMEOUT_SECONDS + 1),
            MAX_TIMEOUT_SECONDS
        );
    }

    fn installer_with_fake_pip(temp_dir: &TempDir, pip_script: String) -> PythonInstaller {
        let venv_root = temp_dir.path().join("venvs");
        let manager = VenvManager::new(&venv_root);
        let bin_dir = manager.venv_path("test").join("bin");
        fs::create_dir_all(&bin_dir).expect("bin dir");
        fs::write(bin_dir.join("python"), "#!/usr/bin/env sh\nexit 0\n").expect("python shim");
        write_executable(&bin_dir.join("pip"), &pip_script);
        PythonInstaller::new(manager, temp_dir.path().join("experiments"))
    }

    fn write_executable(path: &Path, content: &str) {
        fs::write(path, content).expect("write executable");
        let permissions = fs::Permissions::from_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod executable");
    }

    fn success_pip_script(args_file: &str) -> String {
        format!(
            "#!/usr/bin/env sh\nprintf '%s\\n' \"$@\" > '{args_file}'\nprintf 'Successfully installed demo-1.0.0\\n'\n"
        )
    }

    fn sleeping_pip_script() -> String {
        "#!/usr/bin/env sh\nsleep 2\nprintf 'Successfully installed demo-1.0.0\\n'\n".to_string()
    }
}

use crate::venv::{PackageInfo, VenvManager};
use serde::{Deserialize, Serialize};
use std::process::Output;
use std::time::{Duration, Instant};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct PythonInstaller {
    manager: VenvManager,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PythonInstallArgs {
    #[serde(default)]
    pub packages: Vec<String>,
    pub venv: String,
    #[serde(default)]
    pub requirements_file: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InstallResult {
    pub installed: Vec<String>,
    pub duration_ms: u64,
}

impl PythonInstaller {
    #[must_use]
    pub fn new(manager: VenvManager) -> Self {
        Self { manager }
    }

    pub async fn install(&self, args: PythonInstallArgs) -> Result<InstallResult, String> {
        validate_install_request(&args)?;
        self.manager.ensure_venv(&args.venv).await?;

        let started = Instant::now();
        let output = self.run_pip_install(&args).await?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let installed = self.resolve_installed_packages(&args, &stdout).await?;

        Ok(InstallResult {
            installed,
            duration_ms: elapsed_millis(started.elapsed()),
        })
    }

    async fn run_pip_install(&self, args: &PythonInstallArgs) -> Result<Output, String> {
        let mut command = Command::new(self.manager.pip_path(&args.venv));
        configure_install_command(&mut command, args);

        let output = command
            .output()
            .await
            .map_err(|error| format!("failed to run pip install: {error}"))?;
        require_success(output)
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
}

fn validate_install_request(args: &PythonInstallArgs) -> Result<(), String> {
    if args.packages.is_empty() && args.requirements_file.is_none() {
        return Err("python_install requires 'packages' or 'requirements_file'".to_string());
    }

    Ok(())
}

fn configure_install_command(command: &mut Command, args: &PythonInstallArgs) {
    command.arg("install");
    if let Some(requirements_file) = &args.requirements_file {
        command.arg("-r").arg(requirements_file);
        return;
    }

    command.args(args.packages.iter().map(String::as_str));
}

fn require_success(output: Output) -> Result<Output, String> {
    if output.status.success() {
        return Ok(output);
    }

    Err(format!(
        "pip install failed: {}",
        format_process_output(&output)
    ))
}

fn format_process_output(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    format!("status {:?}; {detail}", output.status.code())
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

fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

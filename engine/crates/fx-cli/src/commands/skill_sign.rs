use anyhow::{Context, Result};
use clap::Args;
use fx_skills::manifest::parse_manifest;
use fx_skills::signing::sign_skill;
use std::fs;
use std::path::{Path, PathBuf};

const SIGNING_KEY_RELATIVE_PATH: &str = "keys/signing_key.pem";
const SLASH_SIGN_USAGE: &str = "Usage: /sign <skill> | /sign --all";

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct SignArgs {
    /// Skill name
    #[arg(value_name = "SKILL", required_unless_present = "all")]
    skill: Option<String>,
    /// Sign all installed skills
    #[arg(long, conflicts_with = "skill")]
    all: bool,
    /// Override data directory (default: configured data dir or ~/.fawx)
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SignSelection {
    Skill(String),
    All,
}

#[derive(Debug, Default)]
struct BatchSignReport {
    signed: Vec<SignedSkill>,
    failed: Vec<String>,
}

#[derive(Debug)]
struct SignedSkill {
    name: String,
    signature_path: PathBuf,
}

#[derive(Debug)]
struct SkillArtifact {
    name: String,
    wasm_path: PathBuf,
    signature_path: PathBuf,
}

#[allow(dead_code)]
impl SignArgs {
    pub(crate) fn selection(&self) -> Result<SignSelection> {
        match (self.skill.as_deref(), self.all) {
            (_, true) => Ok(SignSelection::All),
            (Some(skill), false) => Ok(SignSelection::Skill(skill.to_string())),
            (None, false) => Err(anyhow::anyhow!("missing skill name or --all")),
        }
    }

    fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }
}

#[allow(dead_code)]
pub fn run(args: &SignArgs) -> Result<()> {
    let output = sign_output(args.selection()?, args.data_dir())?;
    println!("{output}");
    Ok(())
}

pub(crate) fn sign_output(selection: SignSelection, data_dir: Option<&Path>) -> Result<String> {
    let root = resolve_data_dir(data_dir);
    match selection {
        SignSelection::Skill(name) => sign_single_skill(&root, &name),
        SignSelection::All => sign_all_skills(&root),
    }
}

pub(crate) fn parse_slash_selection(
    target: Option<&str>,
    has_extra_args: bool,
) -> Result<SignSelection> {
    if has_extra_args {
        return Err(anyhow::anyhow!(SLASH_SIGN_USAGE));
    }
    match target {
        Some("--all") => Ok(SignSelection::All),
        Some(skill) => Ok(SignSelection::Skill(skill.to_string())),
        None => Err(anyhow::anyhow!(SLASH_SIGN_USAGE)),
    }
}

pub(crate) fn slash_help_lines() -> [&'static str; 2] {
    [
        "  /sign <skill>   Sign one installed WASM skill",
        "  /sign --all     Sign all installed WASM skills",
    ]
}

fn sign_single_skill(data_dir: &Path, requested_name: &str) -> Result<String> {
    validate_requested_skill_name(requested_name)?;
    let key_bytes = load_signing_key(data_dir)
        .map_err(|error| anyhow::anyhow!("Failed to sign skill '{requested_name}': {error:#}"))?;
    let skill_dir = data_dir.join("skills").join(requested_name);
    let signed =
        sign_skill_dir(&skill_dir, &key_bytes).map_err(|error| anyhow::anyhow!("{error:#}"))?;
    Ok(render_single_success(&signed))
}

fn sign_all_skills(data_dir: &Path) -> Result<String> {
    let skill_dirs = installed_skill_dirs(&data_dir.join("skills"))?;
    if skill_dirs.is_empty() {
        return Ok("No installed skills to sign.".to_string());
    }
    let report = match load_signing_key(data_dir) {
        Ok(key_bytes) => sign_each_skill(&skill_dirs, &key_bytes),
        Err(error) => sign_each_skill_key_error(&skill_dirs, &error),
    };
    report.into_result()
}

fn validate_requested_skill_name(name: &str) -> Result<()> {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        anyhow::bail!("Invalid skill name: must not contain path separators or '..'");
    }
    Ok(())
}

fn resolve_data_dir(data_dir: Option<&Path>) -> PathBuf {
    let Some(data_dir) = data_dir else {
        return configured_data_dir();
    };
    data_dir.to_path_buf()
}

fn configured_data_dir() -> PathBuf {
    let base = crate::startup::fawx_data_dir();
    let config = crate::startup::load_config().unwrap_or_default();
    crate::startup::configured_data_dir(&base, &config)
}

fn load_signing_key(data_dir: &Path) -> Result<Vec<u8>> {
    let key_path = data_dir.join(SIGNING_KEY_RELATIVE_PATH);
    fs::read(&key_path).with_context(|| format!("Signing key not found at {}", key_path.display()))
}

fn installed_skill_dirs(skills_dir: &Path) -> Result<Vec<PathBuf>> {
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }
    let mut dirs = Vec::new();
    for entry in fs::read_dir(skills_dir).context("Failed to read installed skills directory")? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn sign_each_skill(skill_dirs: &[PathBuf], key_bytes: &[u8]) -> BatchSignReport {
    let mut report = BatchSignReport::default();
    for skill_dir in skill_dirs {
        report.record(sign_skill_dir(skill_dir, key_bytes));
    }
    report
}

fn sign_each_skill_key_error(skill_dirs: &[PathBuf], error: &anyhow::Error) -> BatchSignReport {
    let mut report = BatchSignReport::default();
    for skill_dir in skill_dirs {
        let label = display_skill_dir_name(skill_dir);
        report
            .failed
            .push(format!("Failed to sign skill '{label}': {error:#}"));
    }
    report
}

fn sign_skill_dir(skill_dir: &Path, key_bytes: &[u8]) -> Result<SignedSkill> {
    let label = display_skill_dir_name(skill_dir);
    let artifact = load_skill_artifact(skill_dir)
        .with_context(|| format!("Failed to sign skill '{label}'"))?;
    let name = artifact.name.clone();
    sign_loaded_artifact(&artifact, key_bytes)
        .with_context(|| format!("Failed to sign skill '{name}'"))
}

fn display_skill_dir_name(skill_dir: &Path) -> String {
    skill_dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| skill_dir.display().to_string())
}

fn load_skill_artifact(skill_dir: &Path) -> Result<SkillArtifact> {
    let manifest_path = skill_dir.join("manifest.toml");
    let manifest = load_manifest_name(&manifest_path)?;
    let wasm_path = skill_dir.join(format!("{}.wasm", manifest));
    ensure_skill_file(&wasm_path, "WASM file")?;
    Ok(SkillArtifact {
        signature_path: skill_dir.join(format!("{}.wasm.sig", manifest)),
        name: manifest,
        wasm_path,
    })
}

fn load_manifest_name(manifest_path: &Path) -> Result<String> {
    let content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read manifest {}", manifest_path.display()))?;
    let manifest = parse_manifest(&content)
        .map_err(|error| anyhow::anyhow!("Failed to parse manifest: {error}"))?;
    Ok(manifest.name)
}

fn ensure_skill_file(path: &Path, label: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    anyhow::bail!("{label} not found at {}", path.display());
}

fn sign_loaded_artifact(artifact: &SkillArtifact, key_bytes: &[u8]) -> Result<SignedSkill> {
    let wasm_bytes = fs::read(&artifact.wasm_path)
        .with_context(|| format!("Failed to read {}", artifact.wasm_path.display()))?;
    let signature = sign_skill(&wasm_bytes, key_bytes)
        .map_err(|error| anyhow::anyhow!("Failed to sign WASM bytes: {error}"))?;
    fs::write(&artifact.signature_path, signature)
        .with_context(|| format!("Failed to write {}", artifact.signature_path.display()))?;
    Ok(SignedSkill {
        name: artifact.name.clone(),
        signature_path: artifact.signature_path.clone(),
    })
}

fn render_single_success(signed: &SignedSkill) -> String {
    format!(
        "Signed skill '{}'\n  Signature: {}",
        signed.name,
        signed.signature_path.display()
    )
}

impl BatchSignReport {
    fn record(&mut self, result: Result<SignedSkill>) {
        match result {
            Ok(signed) => self.signed.push(signed),
            Err(error) => self.failed.push(format!("{error:#}")),
        }
    }

    fn into_result(self) -> Result<String> {
        let rendered = self.render();
        if self.failed.is_empty() {
            return Ok(rendered);
        }
        Err(anyhow::anyhow!(rendered))
    }

    fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.extend(self.signed.iter().map(render_single_success));
        lines.extend(self.failed.iter().cloned());
        lines.push(format!("Signed {} skill(s).", self.signed.len()));
        if !self.failed.is_empty() {
            lines.push(format!("Failed {} skill(s).", self.failed.len()));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_skills::signing::{generate_keypair, verify_skill};
    use tempfile::TempDir;

    fn write_signing_key(temp: &TempDir) -> Vec<u8> {
        let (private_key, public_key) = generate_keypair().expect("generate keypair");
        let keys_dir = temp.path().join("keys");
        fs::create_dir_all(&keys_dir).expect("create keys dir");
        fs::write(keys_dir.join("signing_key.pem"), &private_key).expect("write signing key");
        public_key
    }

    fn install_skill(temp: &TempDir, name: &str, wasm_bytes: &[u8]) {
        let skill_dir = temp.path().join("skills").join(name);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("manifest.toml"),
            format!(
                "name = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"test\"\nauthor = \"tester\"\napi_version = \"host_api_v1\"\ncapabilities = []\n"
            ),
        )
        .expect("write manifest");
        fs::write(skill_dir.join(format!("{name}.wasm")), wasm_bytes).expect("write wasm");
    }

    fn signature_bytes(temp: &TempDir, name: &str) -> Vec<u8> {
        fs::read(
            temp.path()
                .join("skills")
                .join(name)
                .join(format!("{name}.wasm.sig")),
        )
        .expect("read signature")
    }

    #[test]
    fn sign_output_signs_one_installed_skill() {
        let temp = TempDir::new().expect("tempdir");
        let public_key = write_signing_key(&temp);
        install_skill(&temp, "weather", b"weather-wasm");

        let output = sign_output(
            SignSelection::Skill("weather".to_string()),
            Some(temp.path()),
        )
        .expect("sign");

        assert!(output.contains("Signed skill 'weather'"));
        let signature = signature_bytes(&temp, "weather");
        let valid = verify_skill(b"weather-wasm", &signature, &public_key).expect("verify");
        assert!(valid);
    }

    #[test]
    fn sign_output_signs_all_installed_skills() {
        let temp = TempDir::new().expect("tempdir");
        let public_key = write_signing_key(&temp);
        install_skill(&temp, "weather", b"weather-wasm");
        install_skill(&temp, "github", b"github-wasm");

        let output = sign_output(SignSelection::All, Some(temp.path())).expect("sign all");

        assert!(output.contains("Signed skill 'weather'"));
        assert!(output.contains("Signed skill 'github'"));
        assert!(output.contains("Signed 2 skill(s)."));

        let weather_signature = signature_bytes(&temp, "weather");
        let github_signature = signature_bytes(&temp, "github");
        let weather_valid =
            verify_skill(b"weather-wasm", &weather_signature, &public_key).expect("verify weather");
        let github_valid =
            verify_skill(b"github-wasm", &github_signature, &public_key).expect("verify github");
        assert!(weather_valid);
        assert!(github_valid);
    }

    #[test]
    fn sign_output_names_skill_when_signing_key_is_missing() {
        let temp = TempDir::new().expect("tempdir");
        install_skill(&temp, "weather", b"weather-wasm");

        let error = sign_output(
            SignSelection::Skill("weather".to_string()),
            Some(temp.path()),
        )
        .expect_err("missing key");

        assert!(error.to_string().contains("Failed to sign skill 'weather'"));
        assert!(error.to_string().contains("Signing key not found"));
    }

    #[test]
    fn sign_all_names_each_skill_when_signing_key_is_missing() {
        let temp = TempDir::new().expect("tempdir");
        install_skill(&temp, "weather", b"weather-wasm");
        install_skill(&temp, "github", b"github-wasm");

        let error = sign_output(SignSelection::All, Some(temp.path())).expect_err("missing key");

        assert!(error.to_string().contains("Failed to sign skill 'weather'"));
        assert!(error.to_string().contains("Failed to sign skill 'github'"));
    }

    #[test]
    fn parse_slash_selection_matches_documented_surface() {
        assert_eq!(
            parse_slash_selection(Some("weather"), false).expect("single skill"),
            SignSelection::Skill("weather".to_string())
        );
        assert_eq!(
            parse_slash_selection(Some("--all"), false).expect("all skills"),
            SignSelection::All
        );
        assert_eq!(
            parse_slash_selection(None, false)
                .expect_err("missing target")
                .to_string(),
            SLASH_SIGN_USAGE
        );
    }

    #[test]
    fn parse_slash_selection_rejects_extra_args() {
        assert_eq!(
            parse_slash_selection(Some("weather"), true)
                .expect_err("extra args for single skill")
                .to_string(),
            SLASH_SIGN_USAGE
        );
        assert_eq!(
            parse_slash_selection(Some("--all"), true)
                .expect_err("extra args for all skills")
                .to_string(),
            SLASH_SIGN_USAGE
        );
    }
}

use anyhow::anyhow;
use fx_skills::manifest::parse_manifest;
use fx_skills::signing::verify_skill;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillSignatureReport {
    pub verified: Vec<String>,
    pub unsigned: Vec<String>,
    pub invalid: Vec<String>,
    pub unverified: Vec<String>,
}

impl SkillSignatureReport {
    pub fn installed_count(&self) -> usize {
        self.verified.len() + self.unsigned.len() + self.invalid.len() + self.unverified.len()
    }
}

pub fn scan_skill_signatures(
    skills_dir: &Path,
    trusted_keys_dir: &Path,
) -> anyhow::Result<SkillSignatureReport> {
    let trusted_keys = load_trusted_keys(trusted_keys_dir)?;
    let mut report = SkillSignatureReport::default();
    for skill_dir in skill_dirs(skills_dir)? {
        let skill = load_skill_files(&skill_dir)?;
        classify_skill(&mut report, skill, &trusted_keys)?;
    }
    sort_report(&mut report);
    Ok(report)
}

fn load_trusted_keys(keys_dir: &Path) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut keys = Vec::new();
    if !keys_dir.exists() {
        return Ok(keys);
    }
    for entry in fs::read_dir(keys_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("pub") {
            continue;
        }
        let key = fs::read(&path)?;
        if key.len() == 32 {
            keys.push(key);
        }
    }
    Ok(keys)
}

fn skill_dirs(skills_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    if !skills_dir.exists() {
        return Ok(dirs);
    }
    for entry in fs::read_dir(skills_dir)? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

struct SkillFiles {
    name: String,
    wasm: Vec<u8>,
    signature: Option<Vec<u8>>,
}

fn load_skill_files(skill_dir: &Path) -> anyhow::Result<SkillFiles> {
    let manifest = read_manifest(skill_dir)?;
    let wasm = fs::read(skill_dir.join(format!("{}.wasm", manifest.name)))?;
    let signature = read_signature(skill_dir, &manifest.name)?;
    Ok(SkillFiles {
        name: manifest.name,
        wasm,
        signature,
    })
}

fn read_manifest(skill_dir: &Path) -> anyhow::Result<fx_skills::manifest::SkillManifest> {
    let content = fs::read_to_string(skill_dir.join("manifest.toml"))?;
    parse_manifest(&content).map_err(|error| anyhow!(error))
}

fn read_signature(skill_dir: &Path, skill_name: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let path = skill_dir.join(format!("{skill_name}.wasm.sig"));
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(anyhow!(error)),
    }
}

fn classify_skill(
    report: &mut SkillSignatureReport,
    skill: SkillFiles,
    trusted_keys: &[Vec<u8>],
) -> anyhow::Result<()> {
    let Some(signature) = skill.signature.as_deref() else {
        report.unsigned.push(skill.name);
        return Ok(());
    };
    if trusted_keys.is_empty() {
        report.unverified.push(skill.name);
        return Ok(());
    }
    if signature_matches_any_key(&skill.wasm, signature, trusted_keys)? {
        report.verified.push(skill.name);
    } else {
        report.invalid.push(skill.name);
    }
    Ok(())
}

fn signature_matches_any_key(
    wasm: &[u8],
    signature: &[u8],
    trusted_keys: &[Vec<u8>],
) -> anyhow::Result<bool> {
    for key in trusted_keys {
        if verify_skill(wasm, signature, key)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn sort_report(report: &mut SkillSignatureReport) {
    report.verified.sort();
    report.unsigned.sort();
    report.invalid.sort();
    report.unverified.sort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tempfile::TempDir;

    fn write_skill(skill_dir: &Path, name: &str, wasm: &[u8]) {
        fs::create_dir_all(skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("manifest.toml"),
            format!(
                "name = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"test\"\nauthor = \"tester\"\napi_version = \"host_api_v1\"\ncapabilities = []\n"
            ),
        )
        .expect("write manifest");
        fs::write(skill_dir.join(format!("{name}.wasm")), wasm).expect("write wasm");
    }

    fn write_keypair(keys_dir: &Path) -> Vec<u8> {
        fs::create_dir_all(keys_dir).expect("create keys dir");
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .expect("generate pkcs8");
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("keypair");
        fs::write(keys_dir.join("trusted.pub"), pair.public_key().as_ref()).expect("write pubkey");
        pkcs8.as_ref().to_vec()
    }

    fn sign(wasm: &[u8], pkcs8: &[u8]) -> Vec<u8> {
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8).expect("pkcs8");
        pair.sign(wasm).as_ref().to_vec()
    }

    #[test]
    fn scan_skill_signatures_reports_unsigned_skill() {
        let temp = TempDir::new().expect("tempdir");
        let skills_dir = temp.path().join("skills");
        let keys_dir = temp.path().join("keys");
        write_skill(&skills_dir.join("weather"), "weather", b"wasm");

        let report = scan_skill_signatures(&skills_dir, &keys_dir).expect("scan");
        assert_eq!(report.unsigned, vec!["weather".to_string()]);
    }

    #[test]
    fn scan_skill_signatures_reports_valid_and_invalid_signatures() {
        let temp = TempDir::new().expect("tempdir");
        let skills_dir = temp.path().join("skills");
        let keys_dir = temp.path().join("keys");
        let pkcs8 = write_keypair(&keys_dir);
        write_skill(&skills_dir.join("valid"), "valid", b"valid-wasm");
        write_skill(&skills_dir.join("invalid"), "invalid", b"invalid-wasm");
        let valid_sig = sign(b"valid-wasm", &pkcs8);
        fs::write(skills_dir.join("valid").join("valid.wasm.sig"), valid_sig).expect("valid sig");
        fs::write(
            skills_dir.join("invalid").join("invalid.wasm.sig"),
            vec![0u8; 64],
        )
        .expect("invalid sig");

        let report = scan_skill_signatures(&skills_dir, &keys_dir).expect("scan");
        assert_eq!(report.verified, vec!["valid".to_string()]);
        assert_eq!(report.invalid, vec!["invalid".to_string()]);
    }
}

use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run_cert(hostname: Option<String>, data_dir: &Path) -> anyhow::Result<()> {
    let hostname = match hostname.and_then(normalize_hostname) {
        Some(hostname) => hostname,
        None => detect_tailscale_hostname()?,
    };

    println!("Generating TLS certificate for {hostname}...");
    let (cert_path, key_path) = generate_cert_files(&hostname, data_dir)?;
    println!("✅ Certificate: {}", cert_path.display());
    println!("✅ Key: {}", key_path.display());
    println!("\nHTTPS is ready. Restart the server to use the new certificate.");
    Ok(())
}

pub(crate) fn generate_cert_files(
    hostname: &str,
    data_dir: &Path,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let (cert_path, key_path) = cert_paths(data_dir);
    if let Some(tls_dir) = cert_path.parent() {
        std::fs::create_dir_all(tls_dir).context("failed to create TLS directory")?;
    }
    run_tailscale_cert(hostname, &cert_path, &key_path)?;
    Ok((cert_path, key_path))
}

pub(crate) fn cert_paths(data_dir: &Path) -> (PathBuf, PathBuf) {
    let tls_dir = data_dir.join("tls");
    (tls_dir.join("cert.pem"), tls_dir.join("key.pem"))
}

fn run_tailscale_cert(hostname: &str, cert_path: &Path, key_path: &Path) -> anyhow::Result<()> {
    let output = Command::new("tailscale")
        .arg("cert")
        .arg("--cert-file")
        .arg(cert_path)
        .arg("--key-file")
        .arg(key_path)
        .arg("--")
        .arg(hostname)
        .output()
        .context("failed to run tailscale cert")?;

    if output.status.success() {
        return Ok(());
    }

    Err(map_tailscale_cert_error(&output.stderr))
}

fn map_tailscale_cert_error(stderr: &[u8]) -> anyhow::Error {
    let stderr = String::from_utf8_lossy(stderr);
    if stderr.contains("not logged in") {
        return anyhow!("Tailscale is not logged in. Run 'tailscale login' first.");
    }
    if stderr.contains("HTTPS certificates are not available") {
        return anyhow!("HTTPS certificates are not enabled for this tailnet.");
    }
    anyhow!("tailscale cert failed: {stderr}")
}

fn detect_tailscale_hostname() -> anyhow::Result<String> {
    let output = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .context("failed to run tailscale status")?;

    if !output.status.success() {
        return Err(anyhow!("tailscale status failed — is Tailscale running?"));
    }

    parse_dns_name(&output.stdout)
}

fn parse_dns_name(status_stdout: &[u8]) -> anyhow::Result<String> {
    let json: serde_json::Value =
        serde_json::from_slice(status_stdout).context("failed to parse tailscale status")?;

    json.pointer("/Self/DNSName")
        .and_then(|value| value.as_str())
        .and_then(|value| normalize_hostname(value.to_string()))
        .ok_or_else(|| anyhow!("could not determine Tailscale hostname"))
}

fn normalize_hostname(hostname: String) -> Option<String> {
    let hostname = hostname.trim().trim_end_matches('.').to_string();
    (!hostname.is_empty()).then_some(hostname)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cert_paths_use_tls_directory() {
        let data_dir = Path::new("/tmp/fawx");
        let (cert_path, key_path) = cert_paths(data_dir);

        assert_eq!(cert_path, data_dir.join("tls").join("cert.pem"));
        assert_eq!(key_path, data_dir.join("tls").join("key.pem"));
    }

    #[test]
    fn parse_dns_name_trims_trailing_dot() {
        let hostname = parse_dns_name(br#"{"Self":{"DNSName":"fawx.tail123.ts.net."}}"#)
            .expect("hostname should parse");

        assert_eq!(hostname, "fawx.tail123.ts.net");
    }

    #[test]
    fn parse_dns_name_rejects_missing_dns_name() {
        let error = parse_dns_name(br#"{"Self":{}}"#).expect_err("hostname should be required");

        assert!(error
            .to_string()
            .contains("could not determine Tailscale hostname"));
    }

    #[test]
    fn normalize_hostname_rejects_blank_values() {
        assert_eq!(normalize_hostname("   ".to_string()), None);
    }
}

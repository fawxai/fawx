use crate::error::HttpError;
use std::net::IpAddr;
use std::process::Command;

const TAILSCALE_CLI_PATHS: [&str; 4] = [
    "tailscale",
    "/opt/homebrew/bin/tailscale",
    "/usr/local/bin/tailscale",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
];

const NO_TAILSCALE_MESSAGE: &str = "Could not detect Tailscale interface.\n\
     The HTTP server will continue with a localhost-only binding.";

#[derive(Debug)]
enum ScanOutcome {
    Found(IpAddr),
    NoMatch,
    Failed,
}

pub fn is_tailscale_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 100 && (octets[1] & 0xC0) == 64
        }
        _ => false,
    }
}

pub fn detect_tailscale_ip() -> Result<IpAddr, HttpError> {
    if let Some(ip) = detect_via_tailscale_cli() {
        return Ok(ip);
    }
    detect_via_cgnat_scan()
}

pub fn detect_via_tailscale_cli() -> Option<IpAddr> {
    TAILSCALE_CLI_PATHS
        .iter()
        .find_map(|path| detect_tailscale_ip_from_binary(path))
}

fn detect_tailscale_ip_from_binary(binary: &str) -> Option<IpAddr> {
    let output = Command::new(binary).args(["ip", "-4"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_tailscale_cli_output(&output.stdout)
}

fn parse_tailscale_cli_output(stdout: &[u8]) -> Option<IpAddr> {
    let text = String::from_utf8_lossy(stdout);
    let ip: IpAddr = text.trim().parse().ok()?;
    is_tailscale_ip(&ip).then_some(ip)
}

pub fn detect_via_cgnat_scan() -> Result<IpAddr, HttpError> {
    match scan_for_tailscale_ip("ip", &["-4", "-o", "addr", "show"]) {
        ScanOutcome::Found(ip) => Ok(ip),
        ScanOutcome::NoMatch if !cfg!(target_os = "macos") => no_tailscale_error(),
        ScanOutcome::NoMatch | ScanOutcome::Failed => detect_via_ifconfig_scan(),
    }
}

fn detect_via_ifconfig_scan() -> Result<IpAddr, HttpError> {
    match scan_for_tailscale_ip("/sbin/ifconfig", &[]) {
        ScanOutcome::Found(ip) => Ok(ip),
        ScanOutcome::NoMatch | ScanOutcome::Failed => no_tailscale_error(),
    }
}

fn no_tailscale_error() -> Result<IpAddr, HttpError> {
    Err(HttpError::NoTailscale(NO_TAILSCALE_MESSAGE.to_string()))
}

fn scan_for_tailscale_ip(command: &str, args: &[&str]) -> ScanOutcome {
    let output = match Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => output,
        _ => return ScanOutcome::Failed,
    };

    match find_cgnat_ip(&String::from_utf8_lossy(&output.stdout)) {
        Some(ip) => ScanOutcome::Found(ip),
        None => ScanOutcome::NoMatch,
    }
}

fn find_cgnat_ip(text: &str) -> Option<IpAddr> {
    text.lines()
        .filter_map(extract_ip_from_line)
        .find(is_tailscale_ip)
}

pub fn extract_ip_from_line(line: &str) -> Option<IpAddr> {
    let inet_pos = line.find("inet ")?;
    let after_inet = &line[inet_pos + 5..];
    let addr_token = after_inet.split_whitespace().next()?;
    let addr_str = addr_token.split('/').next()?;
    addr_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn parse_tailscale_cli_output_returns_cgnat_ip() {
        let stdout = b"100.100.1.1\n";

        assert_eq!(
            parse_tailscale_cli_output(stdout),
            Some(IpAddr::V4(Ipv4Addr::new(100, 100, 1, 1)))
        );
    }

    #[test]
    fn parse_tailscale_cli_output_rejects_non_cgnat_ip() {
        let stdout = b"192.168.1.10\n";

        assert_eq!(parse_tailscale_cli_output(stdout), None);
    }

    #[test]
    fn parse_tailscale_cli_output_rejects_garbage() {
        let stdout = b"definitely not an ip\n";

        assert_eq!(parse_tailscale_cli_output(stdout), None);
    }

    #[test]
    fn parse_macos_ifconfig_line_extracts_cgnat_ip() {
        let line = "inet 100.100.2.1 --> 100.100.2.1 netmask 0xffffffff";

        assert_eq!(
            extract_ip_from_line(line),
            Some(IpAddr::V4(Ipv4Addr::new(100, 100, 2, 1)))
        );
    }

    #[test]
    fn parse_macos_ifconfig_line_without_inet_prefix_returns_none() {
        let line = "10.0.0.5 --> 10.0.0.5 netmask 0xffffffff";

        assert_eq!(extract_ip_from_line(line), None);
    }

    #[test]
    fn parse_macos_ifconfig_line_ignores_non_cgnat() {
        let text = "utun3: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1380\n\
                    \tinet 192.168.1.10 --> 192.168.1.10 netmask 0xffffffff";

        assert_eq!(find_cgnat_ip(text), None);
    }

    #[test]
    fn linux_ip_output_still_parsed_correctly() {
        let text = "7: tailscale0    inet 100.100.1.1/32 brd 100.100.1.1 scope global tailscale0";

        assert_eq!(
            find_cgnat_ip(text),
            Some(IpAddr::V4(Ipv4Addr::new(100, 100, 1, 1)))
        );
    }
}

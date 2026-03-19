use crate::error::HttpError;
use std::net::IpAddr;

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
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let ip: IpAddr = text.trim().parse().ok()?;
    if is_tailscale_ip(&ip) {
        Some(ip)
    } else {
        None
    }
}

pub fn detect_via_cgnat_scan() -> Result<IpAddr, HttpError> {
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .map_err(|e| HttpError::NoTailscale(format!("failed to run `ip addr`: {e}")))?;

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(ip) = extract_ip_from_line(line) {
            if is_tailscale_ip(&ip) {
                return Ok(ip);
            }
        }
    }

    Err(HttpError::NoTailscale(
        "Could not detect Tailscale interface.\n\
         The HTTP server will continue with a localhost-only binding."
            .to_string(),
    ))
}

pub fn extract_ip_from_line(line: &str) -> Option<IpAddr> {
    let inet_pos = line.find("inet ")?;
    let after_inet = &line[inet_pos + 5..];
    let addr_str = after_inet.split('/').next()?;
    addr_str.trim().parse().ok()
}

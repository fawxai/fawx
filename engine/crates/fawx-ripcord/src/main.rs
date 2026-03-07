use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::{env, fs, process};

use serde::{Deserialize, Serialize};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_SNAPSHOTS: usize = 5;

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    created_at: String,
    fawx_binary_hash: String,
    files: Vec<String>,
    reason: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let data_dir = resolve_data_dir();
    let snapshots_dir = data_dir.join("snapshots");

    match args.get(1).map(String::as_str) {
        None => show_status(&data_dir, &snapshots_dir),
        Some("--list") => list_snapshots(&snapshots_dir),
        Some("--create") => create_snapshot(&data_dir, &snapshots_dir),
        Some("restore") => handle_restore(&args, &data_dir, &snapshots_dir),
        Some(flag) => {
            eprintln!("unknown flag: {flag}");
            eprintln!("usage: fawx-ripcord [--list | --create | restore [--yes] [--snap <id>]]");
            process::exit(1);
        }
    }
}

fn handle_restore(args: &[String], data_dir: &Path, snapshots_dir: &Path) {
    let force = args.iter().any(|a| a == "--yes");
    let snap_id = args
        .windows(2)
        .find(|w| w[0] == "--snap")
        .map(|w| w[1].as_str());

    let snapshot_dir = match snap_id {
        Some(id) => snapshots_dir.join(id),
        None => find_latest_snapshot(snapshots_dir),
    };

    if !snapshot_dir.exists() {
        eprintln!("snapshot not found: {}", snapshot_dir.display());
        process::exit(1);
    }

    if !force {
        confirm_restore(&snapshot_dir);
    }
    restore_snapshot(data_dir, &snapshot_dir);
}

fn resolve_data_dir() -> PathBuf {
    if let Ok(dir) = env::var("FAWX_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".fawx")
}

fn show_status(data_dir: &Path, snapshots_dir: &Path) {
    println!("fawx-ripcord v{VERSION}\n");
    println!("Current state:");
    print_file_status(data_dir);
    println!();
    print_snapshot_summary(snapshots_dir);
    print_usage();
}

fn print_file_status(data_dir: &Path) {
    let config = data_dir.join("config.toml");
    if config.exists() {
        if let Ok(meta) = fs::metadata(&config) {
            println!("  Config: {} ({} bytes)", config.display(), meta.len());
        }
    } else {
        println!("  Config: not found");
    }

    let skills_dir = data_dir.join("skills");
    if skills_dir.is_dir() {
        let count = fs::read_dir(&skills_dir)
            .map(|rd| rd.filter(|e| e.is_ok()).count())
            .unwrap_or(0);
        println!("  Skills: {count} installed");
    } else {
        println!("  Skills: none");
    }
}

fn print_snapshot_summary(snapshots_dir: &Path) {
    let snapshots = sorted_snapshot_dirs(snapshots_dir);
    if snapshots.is_empty() {
        println!("No snapshots available.");
        return;
    }
    println!("Available snapshots:");
    for (i, snap) in snapshots.iter().enumerate() {
        let name = snap
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let file_count = count_snapshot_files(snap);
        let reason = read_manifest_reason(snap);
        println!(
            "  [{}] {}  ({} files, reason: {})",
            i + 1,
            name,
            file_count,
            reason
        );
    }
}

fn print_usage() {
    println!("\nUsage:");
    println!("  fawx-ripcord restore           Restore latest snapshot (with confirmation)");
    println!("  fawx-ripcord --create          Create a new snapshot");
    println!("  fawx-ripcord restore --snap 1  Restore a specific snapshot");
}

fn list_snapshots(snapshots_dir: &Path) {
    let snapshots = sorted_snapshot_dirs(snapshots_dir);
    if snapshots.is_empty() {
        println!("No snapshots found.");
        return;
    }
    println!("Snapshots in {}:\n", snapshots_dir.display());
    for snap in &snapshots {
        let name = snap
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let manifest_path = snap.join("manifest.json");
        if let Ok(data) = fs::read_to_string(&manifest_path) {
            if let Ok(m) = serde_json::from_str::<Manifest>(&data) {
                println!(
                    "  {} — {} files, reason: {}",
                    m.created_at,
                    m.files.len(),
                    m.reason
                );
                continue;
            }
        }
        println!("  {name} — manifest missing or corrupt");
    }
}

fn create_snapshot(data_dir: &Path, snapshots_dir: &Path) {
    let timestamp = generate_timestamp();
    let snap_dir = snapshots_dir.join(&timestamp);
    fs::create_dir_all(&snap_dir).unwrap_or_else(|e| {
        eprintln!("failed to create snapshot dir: {e}");
        process::exit(1);
    });

    let mut files = Vec::new();
    copy_config(data_dir, &snap_dir, &mut files);
    copy_skills(data_dir, &snap_dir, &mut files);

    let binary_hash = hash_binary();
    let manifest = Manifest {
        version: 1,
        created_at: timestamp.clone(),
        fawx_binary_hash: binary_hash,
        files,
        reason: "manual".to_string(),
    };
    write_manifest(&snap_dir, &manifest);

    println!("Snapshot saved: {timestamp}");
    prune_snapshots(snapshots_dir, MAX_SNAPSHOTS);
}

fn copy_config(data_dir: &Path, snap_dir: &Path, files: &mut Vec<String>) {
    let config = data_dir.join("config.toml");
    if config.exists() {
        if let Err(e) = fs::copy(&config, snap_dir.join("config.toml")) {
            eprintln!("warning: failed to copy config.toml: {e}");
        } else {
            files.push("config.toml".to_string());
        }
    }
}

fn copy_skills(data_dir: &Path, snap_dir: &Path, files: &mut Vec<String>) {
    let skills_src = data_dir.join("skills");
    if !skills_src.is_dir() {
        return;
    }
    let skills_dst = snap_dir.join("skills");
    copy_dir_recursive(&skills_src, &skills_dst, &skills_src, files);
}

fn copy_dir_recursive(src: &Path, dst: &Path, base: &Path, files: &mut Vec<String>) {
    if let Err(e) = fs::create_dir_all(dst) {
        eprintln!("warning: failed to create dir {}: {e}", dst.display());
        return;
    }
    let entries = match fs::read_dir(src) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("warning: failed to read dir {}: {e}", src.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest, base, files);
        } else if let Err(e) = fs::copy(&path, &dest) {
            eprintln!("warning: failed to copy {}: {e}", path.display());
        } else {
            let rel = path
                .strip_prefix(base)
                .map(|p| format!("skills/{}", p.display()))
                .unwrap_or_else(|_| path.display().to_string());
            files.push(rel);
        }
    }
}

fn write_manifest(snap_dir: &Path, manifest: &Manifest) {
    let json = match serde_json::to_string_pretty(manifest) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("failed to serialize manifest: {e}");
            process::exit(1);
        }
    };
    if let Err(e) = fs::write(snap_dir.join("manifest.json"), json) {
        eprintln!("failed to write manifest.json: {e}");
        process::exit(1);
    }
}

fn restore_snapshot(data_dir: &Path, snapshot_dir: &Path) {
    kill_fawx(data_dir);

    let snap_config = snapshot_dir.join("config.toml");
    if snap_config.exists() {
        let dst = data_dir.join("config.toml");
        if let Err(e) = fs::copy(&snap_config, &dst) {
            eprintln!("failed to restore config.toml: {e}");
            process::exit(1);
        }
        println!("  Restored: config.toml");
    }

    let snap_skills = snapshot_dir.join("skills");
    if snap_skills.is_dir() {
        let dst_skills = data_dir.join("skills");
        if dst_skills.exists() {
            let _ = fs::remove_dir_all(&dst_skills);
        }
        let mut files = Vec::new();
        copy_dir_recursive(&snap_skills, &dst_skills, &snap_skills, &mut files);
        for f in &files {
            println!("  Restored: {f}");
        }
    }

    let snap_name = snapshot_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("\nRestored snapshot: {snap_name}");
    println!("Restart fawx to continue.");
}

fn confirm_restore(snapshot_dir: &Path) {
    let snap_name = snapshot_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    print!("Restore snapshot {snap_name}? Type YES to confirm: ");
    if io::stdout().flush().is_err() {
        process::exit(1);
    }
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() || input.trim() != "YES" {
        println!("Aborted.");
        process::exit(1);
    }
}

fn find_latest_snapshot(snapshots_dir: &Path) -> PathBuf {
    let snapshots = sorted_snapshot_dirs(snapshots_dir);
    match snapshots.last() {
        Some(p) => p.clone(),
        None => {
            eprintln!("no snapshots found in {}", snapshots_dir.display());
            process::exit(1);
        }
    }
}

fn kill_fawx(data_dir: &Path) {
    let pid_file = data_dir.join("fawx.pid");
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            #[cfg(unix)]
            kill_process_by_pid(pid);
            println!("Killed fawx (PID {pid})");
            return;
        }
    }
    // Fall back to pkill
    let _ = process::Command::new("pkill")
        .args(["-f", "fawx serve|fawx$"])
        .status();
}

#[cfg(unix)]
fn kill_process_by_pid(pid: i32) {
    let _ = process::Command::new("kill").arg(pid.to_string()).status();
}

fn prune_snapshots(dir: &Path, keep: usize) {
    let snapshots = sorted_snapshot_dirs(dir);
    if snapshots.len() <= keep {
        return;
    }
    let to_remove = snapshots.len() - keep;
    for snap in snapshots.iter().take(to_remove) {
        if let Err(e) = fs::remove_dir_all(snap) {
            eprintln!(
                "warning: failed to remove old snapshot {}: {e}",
                snap.display()
            );
        }
    }
}

fn hash_binary() -> String {
    let exe = match env::current_exe() {
        Ok(e) => e,
        Err(_) => return "unknown".to_string(),
    };
    let fawx_bin = match exe.parent() {
        Some(parent) => parent.join("fawx"),
        None => return "unknown".to_string(),
    };
    if !fawx_bin.exists() {
        return "unknown".to_string();
    }
    match fs::read(&fawx_bin) {
        Ok(bytes) => format!("{:x}", Sha256::digest(&bytes)),
        Err(_) => "unknown".to_string(),
    }
}

fn sorted_snapshot_dirs(dir: &Path) -> Vec<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

fn count_snapshot_files(snap_dir: &Path) -> usize {
    let manifest_path = snap_dir.join("manifest.json");
    if let Ok(data) = fs::read_to_string(manifest_path) {
        if let Ok(m) = serde_json::from_str::<Manifest>(&data) {
            return m.files.len();
        }
    }
    0
}

fn read_manifest_reason(snap_dir: &Path) -> String {
    let manifest_path = snap_dir.join("manifest.json");
    if let Ok(data) = fs::read_to_string(manifest_path) {
        if let Ok(m) = serde_json::from_str::<Manifest>(&data) {
            return m.reason;
        }
    }
    "unknown".to_string()
}

fn generate_timestamp() -> String {
    // Use system time to generate a UTC-ish timestamp without chrono
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Manual UTC conversion
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_date(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}-{minutes:02}-{seconds:02}")
}

fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    days += 719_468;
    let era = days / 146_097;
    let doe = days % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_data_dir(tmp: &TempDir) -> PathBuf {
        let data = tmp.path().join("data");
        fs::create_dir_all(data.join("skills/test-skill")).expect("create skills dir");
        fs::write(data.join("config.toml"), "key = \"value\"").expect("write config");
        fs::write(data.join("skills/test-skill/test.wasm"), b"fake-wasm-bytes")
            .expect("write wasm");
        data
    }

    fn setup_snapshots_dir(tmp: &TempDir) -> PathBuf {
        let snaps = tmp.path().join("snapshots");
        fs::create_dir_all(&snaps).expect("create snapshots dir");
        snaps
    }

    #[test]
    fn create_snapshot_copies_config() {
        let tmp = TempDir::new().unwrap();
        let data = setup_data_dir(&tmp);
        let snaps = setup_snapshots_dir(&tmp);

        create_snapshot(&data, &snaps);

        let entries = sorted_snapshot_dirs(&snaps);
        assert_eq!(entries.len(), 1);
        let config = entries[0].join("config.toml");
        assert!(config.exists(), "config.toml should exist in snapshot");
        assert_eq!(fs::read_to_string(config).unwrap(), "key = \"value\"");
    }

    #[test]
    fn create_snapshot_copies_skills() {
        let tmp = TempDir::new().unwrap();
        let data = setup_data_dir(&tmp);
        let snaps = setup_snapshots_dir(&tmp);

        create_snapshot(&data, &snaps);

        let entries = sorted_snapshot_dirs(&snaps);
        let wasm = entries[0].join("skills/test-skill/test.wasm");
        assert!(wasm.exists(), "skill wasm should exist in snapshot");
        assert_eq!(fs::read(&wasm).unwrap(), b"fake-wasm-bytes");
    }

    #[test]
    fn create_snapshot_writes_manifest() {
        let tmp = TempDir::new().unwrap();
        let data = setup_data_dir(&tmp);
        let snaps = setup_snapshots_dir(&tmp);

        create_snapshot(&data, &snaps);

        let entries = sorted_snapshot_dirs(&snaps);
        let manifest_path = entries[0].join("manifest.json");
        assert!(manifest_path.exists(), "manifest.json should exist");
        let manifest: Manifest =
            serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.reason, "manual");
        assert!(!manifest.files.is_empty());
    }

    #[test]
    fn restore_snapshot_overwrites_config() {
        let tmp = TempDir::new().unwrap();
        let data = setup_data_dir(&tmp);
        let snaps = setup_snapshots_dir(&tmp);

        create_snapshot(&data, &snaps);

        // Modify the live config
        fs::write(data.join("config.toml"), "corrupted = true").unwrap();

        let entries = sorted_snapshot_dirs(&snaps);
        restore_snapshot(&data, &entries[0]);

        assert_eq!(
            fs::read_to_string(data.join("config.toml")).unwrap(),
            "key = \"value\""
        );
    }

    #[test]
    fn restore_snapshot_overwrites_skills() {
        let tmp = TempDir::new().unwrap();
        let data = setup_data_dir(&tmp);
        let snaps = setup_snapshots_dir(&tmp);

        create_snapshot(&data, &snaps);

        // Corrupt the live skill
        fs::write(data.join("skills/test-skill/test.wasm"), b"corrupted").unwrap();

        let entries = sorted_snapshot_dirs(&snaps);
        restore_snapshot(&data, &entries[0]);

        assert_eq!(
            fs::read(data.join("skills/test-skill/test.wasm")).unwrap(),
            b"fake-wasm-bytes"
        );
    }

    #[test]
    fn list_snapshots_shows_timestamps() {
        let tmp = TempDir::new().unwrap();
        let data = setup_data_dir(&tmp);
        let snaps = setup_snapshots_dir(&tmp);

        create_snapshot(&data, &snaps);

        let entries = sorted_snapshot_dirs(&snaps);
        assert_eq!(entries.len(), 1);

        // Verify the manifest has a created_at timestamp
        let manifest_path = entries[0].join("manifest.json");
        let manifest: Manifest =
            serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
        assert!(
            manifest.created_at.contains('T'),
            "created_at should contain a timestamp with T separator"
        );
    }

    #[test]
    fn prune_keeps_n_newest() {
        let tmp = TempDir::new().unwrap();
        let snaps = setup_snapshots_dir(&tmp);

        // Create 7 fake snapshot dirs
        for i in 0..7 {
            let dir = snaps.join(format!("2026-03-07T0{i}-00-00"));
            fs::create_dir_all(&dir).unwrap();
            let manifest = Manifest {
                version: 1,
                created_at: format!("2026-03-07T0{i}:00:00Z"),
                fawx_binary_hash: "abc".to_string(),
                files: vec![],
                reason: "manual".to_string(),
            };
            fs::write(
                dir.join("manifest.json"),
                serde_json::to_string(&manifest).unwrap(),
            )
            .unwrap();
        }

        prune_snapshots(&snaps, 3);

        let remaining = sorted_snapshot_dirs(&snaps);
        assert_eq!(remaining.len(), 3);
        // Should keep the 3 newest (04, 05, 06)
        let names: Vec<String> = remaining
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "2026-03-07T04-00-00",
                "2026-03-07T05-00-00",
                "2026-03-07T06-00-00",
            ]
        );
    }

    #[test]
    fn restore_latest_picks_newest() {
        let tmp = TempDir::new().unwrap();
        let snaps = setup_snapshots_dir(&tmp);

        // Create snapshots with different timestamps
        for ts in &[
            "2026-03-06T10-00-00",
            "2026-03-07T15-00-00",
            "2026-03-07T02-00-00",
        ] {
            let dir = snaps.join(ts);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("config.toml"), format!("from = \"{ts}\"")).unwrap();
            let manifest = Manifest {
                version: 1,
                created_at: ts.to_string(),
                fawx_binary_hash: "abc".to_string(),
                files: vec!["config.toml".to_string()],
                reason: "manual".to_string(),
            };
            fs::write(
                dir.join("manifest.json"),
                serde_json::to_string(&manifest).unwrap(),
            )
            .unwrap();
        }

        let latest = find_latest_snapshot(&snaps);
        let name = latest.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(
            name, "2026-03-07T15-00-00",
            "should pick the newest by sort"
        );
    }
}

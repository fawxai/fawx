use super::log_files::find_log_files;
use super::runtime_layout::RuntimeLayout;
use chrono::{DateTime, Utc};
use clap::Args;
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Args)]
pub struct LogsArgs {
    /// Number of lines to show from the latest log file
    #[arg(long, default_value_t = 50)]
    pub lines: usize,
    /// List available log files instead of printing the latest file
    #[arg(long)]
    pub list: bool,
}

pub fn run(args: &LogsArgs) -> anyhow::Result<i32> {
    let layout = RuntimeLayout::detect()?;
    if args.list {
        print_log_listing(&layout.logs_dir)?;
        return Ok(0);
    }
    print_latest_log_tail(&layout.logs_dir, args.lines)?;
    Ok(0)
}

fn print_log_listing(logs_dir: &Path) -> anyhow::Result<()> {
    let files = list_log_files(logs_dir)?;
    if files.is_empty() {
        println!("No log files found in {}", logs_dir.display());
        return Ok(());
    }
    println!("Fawx Logs\n─────────\n");
    for file in files {
        println!("{}", format_listing_entry(&file)?);
    }
    Ok(())
}

fn print_latest_log_tail(logs_dir: &Path, lines: usize) -> anyhow::Result<()> {
    let latest = latest_log_file(logs_dir)?;
    let Some(path) = latest else {
        println!("No log files found in {}", logs_dir.display());
        return Ok(());
    };
    println!("==> {} <==", path.display());
    for line in tail_lines(&path, lines)? {
        println!("{line}");
    }
    Ok(())
}

fn list_log_files(logs_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    find_log_files(logs_dir)
}

fn latest_log_file(logs_dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let files = list_log_files(logs_dir)?;
    Ok(files.into_iter().next())
}

fn tail_lines(path: &Path, max_lines: usize) -> anyhow::Result<Vec<String>> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = VecDeque::new();
    for line in reader.lines() {
        push_tail_line(&mut lines, line?, max_lines);
    }
    Ok(lines.into_iter().collect())
}

fn push_tail_line(lines: &mut VecDeque<String>, line: String, max_lines: usize) {
    if max_lines == 0 {
        return;
    }
    if lines.len() == max_lines {
        lines.pop_front();
    }
    lines.push_back(line);
}

fn format_listing_entry(path: &Path) -> anyhow::Result<String> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    let timestamp = format_timestamp(modified);
    let size = metadata.len();
    Ok(format!("{}  {} bytes  {}", timestamp, size, path.display()))
}

fn format_timestamp(time: std::time::SystemTime) -> String {
    DateTime::<Utc>::from(time)
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_log(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create log dir");
        }
        let mut file = fs::File::create(path).expect("create log");
        file.write_all(body.as_bytes()).expect("write log");
    }

    fn set_mtime(path: &Path, unix_secs: i64) {
        use std::time::{Duration, SystemTime};
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(unix_secs as u64);
        let times = fs::FileTimes::new().set_modified(time);
        let file = fs::File::options()
            .write(true)
            .open(path)
            .expect("open for mtime");
        file.set_times(times).expect("set mtime");
    }

    #[test]
    fn latest_log_file_prefers_newest_mtime() {
        let temp = TempDir::new().expect("tempdir");
        let older = temp.path().join("older.log");
        let newer = temp.path().join("newer.log");
        write_log(&older, "older");
        write_log(&newer, "newer");
        set_mtime(&older, 1);
        set_mtime(&newer, 2);

        let latest = latest_log_file(temp.path()).expect("latest");
        assert_eq!(latest, Some(newer));
    }

    #[test]
    fn tail_lines_limits_output_to_requested_count() {
        let temp = TempDir::new().expect("tempdir");
        let log = temp.path().join("fawx.log");
        write_log(&log, "one\ntwo\nthree\nfour\n");

        let lines = tail_lines(&log, 2).expect("tail");
        assert_eq!(lines, vec!["three".to_string(), "four".to_string()]);
    }

    #[test]
    fn latest_log_file_handles_empty_directory() {
        let temp = TempDir::new().expect("tempdir");
        let latest = latest_log_file(temp.path()).expect("latest");
        assert!(latest.is_none());
    }

    #[test]
    fn list_output_format_includes_size_and_path() {
        let temp = TempDir::new().expect("tempdir");
        let log = temp.path().join("fawx.log");
        write_log(&log, "hello");

        let entry = format_listing_entry(&log).expect("entry");
        assert!(entry.contains("bytes"));
        assert!(entry.contains(log.to_string_lossy().as_ref()));
    }
}

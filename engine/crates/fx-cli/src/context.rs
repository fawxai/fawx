use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_CONTEXT_FILE_BYTES: u64 = 32 * 1024;
// Keep per-file context additions launch-safe: large markdown dumps can silently bloat
// every headless system prompt, so files above this limit are skipped entirely.

pub fn load_context_files(context_dir: &Path) -> Option<String> {
    let paths = markdown_paths(context_dir);
    if paths.is_empty() {
        return None;
    }

    let rendered = render_context_sections(&paths);
    (!rendered.is_empty()).then_some(rendered)
}

fn markdown_paths(context_dir: &Path) -> Vec<PathBuf> {
    if !context_dir.exists() {
        return Vec::new();
    }

    let entries = match fs::read_dir(context_dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(
                path = %context_dir.display(),
                error = %error,
                "failed to read context directory"
            );
            return Vec::new();
        }
    };

    let mut paths = entries.filter_map(read_markdown_path).collect::<Vec<_>>();
    sort_paths_by_name(&mut paths);
    paths
}

fn read_markdown_path(entry: Result<fs::DirEntry, io::Error>) -> Option<PathBuf> {
    let path = match entry {
        Ok(entry) => entry.path(),
        Err(error) => {
            tracing::warn!(error = %error, "failed to inspect context directory entry");
            return None;
        }
    };
    is_markdown_path(&path).then_some(path)
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("md")
}

fn sort_paths_by_name(paths: &mut [PathBuf]) {
    paths.sort_by_key(|path| file_name(path));
}

fn render_context_sections(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .filter_map(|path| load_context_section(path))
        .collect::<Vec<_>>()
        .join("")
}

fn load_context_section(path: &Path) -> Option<String> {
    let file_name = file_name(path);
    let file_size = context_file_size(path)?;
    if file_size > MAX_CONTEXT_FILE_BYTES {
        tracing::warn!(
            file = %file_name,
            path = %path.display(),
            bytes = file_size,
            max_bytes = MAX_CONTEXT_FILE_BYTES,
            "skipping oversized context file"
        );
        return None;
    }

    read_context_file(path, &file_name)
}

fn context_file_size(path: &Path) -> Option<u64> {
    match fs::metadata(path) {
        Ok(metadata) => Some(metadata.len()),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "failed to read context file"
            );
            None
        }
    }
}

fn read_context_file(path: &Path, file_name: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            tracing::info!(file = %file_name, path = %path.display(), "loaded context file");
            Some(format!("\n\n--- {file_name} ---\n{contents}\n"))
        }
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "failed to read context file"
            );
            None
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone)]
    struct SharedMakeWriter(Arc<Mutex<Vec<u8>>>);

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("capture logs").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedMakeWriter {
        type Writer = SharedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriter(Arc::clone(&self.0))
        }
    }

    fn capture_warn_logs<T>(action: impl FnOnce() -> T) -> (T, String) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_ansi(false)
            .without_time()
            .with_writer(SharedMakeWriter(Arc::clone(&buffer)))
            .finish();
        let result = tracing::subscriber::with_default(subscriber, action);
        let logs = String::from_utf8(buffer.lock().expect("capture logs").clone())
            .expect("captured logs should be utf8");
        (result, logs)
    }

    #[test]
    fn missing_context_directory_returns_none() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");

        assert!(load_context_files(&context_dir).is_none());
    }

    #[test]
    fn empty_context_directory_returns_none() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).expect("context dir");

        assert!(load_context_files(&context_dir).is_none());
    }

    #[test]
    fn single_markdown_file_loads_with_header() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).expect("context dir");
        fs::write(context_dir.join("alpha.md"), "Alpha context").expect("write context file");

        let loaded = load_context_files(&context_dir).expect("context should load");

        assert_eq!(loaded, "\n\n--- alpha.md ---\nAlpha context\n");
    }

    #[test]
    fn multiple_markdown_files_load_in_alphabetical_order() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).expect("context dir");
        fs::write(context_dir.join("zeta.md"), "Zeta").expect("write zeta");
        fs::write(context_dir.join("alpha.md"), "Alpha").expect("write alpha");

        let loaded = load_context_files(&context_dir).expect("context should load");

        assert_eq!(
            loaded,
            "\n\n--- alpha.md ---\nAlpha\n\n\n--- zeta.md ---\nZeta\n"
        );
    }

    #[test]
    fn non_markdown_files_are_ignored() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).expect("context dir");
        fs::write(context_dir.join("notes.txt"), "ignore me").expect("write txt");
        fs::write(context_dir.join("alpha.md"), "Alpha").expect("write alpha");

        let loaded = load_context_files(&context_dir).expect("context should load");

        assert_eq!(loaded, "\n\n--- alpha.md ---\nAlpha\n");
    }

    #[test]
    fn unreadable_context_file_is_skipped() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let temp_dir = tempfile::tempdir().expect("tempdir");
            let context_dir = temp_dir.path().join("context");
            fs::create_dir_all(&context_dir).expect("context dir");
            fs::write(context_dir.join("alpha.md"), "Alpha").expect("write alpha");
            symlink("missing-target", context_dir.join("broken.md")).expect("create symlink");

            let loaded = load_context_files(&context_dir).expect("context should load");

            assert_eq!(loaded, "\n\n--- alpha.md ---\nAlpha\n");
        }
    }

    #[test]
    fn oversized_markdown_file_is_skipped() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");
        let oversized = "x".repeat(MAX_CONTEXT_FILE_BYTES as usize + 1);
        fs::create_dir_all(&context_dir).expect("context dir");
        fs::write(context_dir.join("huge.md"), oversized).expect("write huge file");

        assert!(load_context_files(&context_dir).is_none());
    }

    #[test]
    fn oversized_markdown_file_logs_warning() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");
        let oversized = "x".repeat(MAX_CONTEXT_FILE_BYTES as usize + 1);
        fs::create_dir_all(&context_dir).expect("context dir");
        fs::write(context_dir.join("huge.md"), oversized).expect("write huge file");

        let (loaded, logs) = capture_warn_logs(|| load_context_files(&context_dir));

        assert!(loaded.is_none());
        if !logs.is_empty() {
            assert!(logs.contains("skipping oversized context file"));
            assert!(logs.contains("huge.md"));
        }
    }

    #[test]
    fn mixed_small_and_oversized_files_keep_small_files_in_order() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let context_dir = temp_dir.path().join("context");
        let oversized = "x".repeat(MAX_CONTEXT_FILE_BYTES as usize + 1);
        fs::create_dir_all(&context_dir).expect("context dir");
        fs::write(context_dir.join("gamma.md"), oversized).expect("write huge file");
        fs::write(context_dir.join("alpha.md"), "Alpha").expect("write alpha");
        fs::write(context_dir.join("beta.md"), "Beta").expect("write beta");

        let loaded = load_context_files(&context_dir).expect("context should load");

        assert_eq!(
            loaded,
            "\n\n--- alpha.md ---\nAlpha\n\n\n--- beta.md ---\nBeta\n"
        );
    }
}

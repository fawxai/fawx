use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[test]
fn imports_all_markdown_files_without_data_loss() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    seed_workspace_markdown_files(source.path());

    let report = execute_import(&test_options(source.path(), data.path())).expect("import");

    assert_eq!(report.copied_count(), 10);
    assert_eq!(read(data.path().join("memory/MEMORY.md")), "long memory");
    assert_eq!(read(data.path().join("memory/2026-03-09.md")), "day one");
    assert_eq!(read(data.path().join("memory/2026-03-10.md")), "day two");
    assert_eq!(read(data.path().join("context/SOUL.md")), "soul");
    assert_eq!(read(data.path().join("context/ENGINEERING.md")), "engineering");
    assert_eq!(read(data.path().join("context/CUSTOM.md")), "custom");
}

#[test]
fn imports_partial_workspace_without_missing_file_noise() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("SOUL.md"), "soul");
    write_file(source.path().join("memory/2026-03-10.md"), "day two");

    let report = execute_import(&test_options(source.path(), data.path())).expect("import");

    assert_eq!(report.copied_count(), 2);
    assert_eq!(report.skipped_count(), 0);
    assert_eq!(read(data.path().join("context/SOUL.md")), "soul");
    assert_eq!(read(data.path().join("memory/2026-03-10.md")), "day two");
}

#[test]
fn imports_unrecognized_root_markdown_files_into_context() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("CUSTOM.md"), "custom");
    write_file(source.path().join("BOOTSTRAP.md"), "bootstrap");

    let report = execute_import(&test_options(source.path(), data.path())).expect("import");

    assert_eq!(report.copied_count(), 2);
    assert_eq!(read(data.path().join("context/CUSTOM.md")), "custom");
    assert_eq!(read(data.path().join("context/BOOTSTRAP.md")), "bootstrap");
}

#[test]
fn preserves_memory_archive_structure_recursively() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(
        source.path().join("memory/archive/2026/03/old.md"),
        "old memory",
    );
    write_file(
        source.path().join("memory/archive/incidents/security.md"),
        "incident",
    );

    let report = execute_import(&test_options(source.path(), data.path())).expect("import");

    assert_eq!(report.copied_count(), 2);
    assert_eq!(
        read(data.path().join("memory/archive/2026/03/old.md")),
        "old memory"
    );
    assert_eq!(
        read(data.path().join("memory/archive/incidents/security.md")),
        "incident"
    );
}

#[test]
fn skips_existing_files_without_force() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("MEMORY.md"), "new memory");
    write_file(data.path().join("memory/MEMORY.md"), "existing memory");

    let report = execute_import(&test_options(source.path(), data.path())).expect("import");

    assert_eq!(report.copied_count(), 0);
    assert_eq!(report.skipped_count(), 1);
    assert_eq!(
        read(data.path().join("memory/MEMORY.md")),
        "existing memory"
    );
}

#[test]
fn force_overwrites_existing_files() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("MEMORY.md"), "new memory");
    write_file(data.path().join("memory/MEMORY.md"), "existing memory");

    let report = execute_import(&ImportOptions {
        force: true,
        ..test_options(source.path(), data.path())
    })
    .expect("import");

    assert_eq!(report.copied_count(), 1);
    assert_eq!(read(data.path().join("memory/MEMORY.md")), "new memory");
}

#[test]
fn dry_run_does_not_copy_files() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("SOUL.md"), "soul");

    let report = execute_import(&ImportOptions {
        dry_run: true,
        ..test_options(source.path(), data.path())
    })
    .expect("import");

    assert_eq!(report.planned_count(), 1);
    assert!(!data.path().join("context/SOUL.md").exists());
}

#[test]
fn missing_source_directory_fails_cleanly() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    let missing = source.path().join("missing");

    let error = execute_import(&test_options(&missing, data.path())).expect_err("error");

    assert!(error.to_string().contains("Directory not found"));
}

#[test]
fn empty_source_directory_fails_cleanly() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");

    let error = execute_import(&test_options(source.path(), data.path())).expect_err("error");

    assert!(error.to_string().contains("No markdown files found"));
}

#[test]
fn root_memory_file_takes_priority_over_dynamic_memory_match() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("MEMORY.md"), "root memory");
    write_file(source.path().join("memory/MEMORY.md"), "nested memory");

    let report = execute_import(&ImportOptions {
        force: true,
        ..test_options(source.path(), data.path())
    })
    .expect("import");

    assert_eq!(report.copied_count(), 1);
    assert_eq!(read(data.path().join("memory/MEMORY.md")), "root memory");
}

#[test]
fn imports_nested_memory_file_when_root_memory_is_absent() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("memory/MEMORY.md"), "nested memory");

    let report = execute_import(&test_options(source.path(), data.path())).expect("import");

    assert_eq!(report.copied_count(), 1);
    assert_eq!(read(data.path().join("memory/MEMORY.md")), "nested memory");
}

#[test]
fn renders_grouped_output_with_summary_and_footer() {
    let source = TempDir::new().expect("tempdir");
    let data = TempDir::new().expect("tempdir");
    write_file(source.path().join("MEMORY.md"), "long memory");
    write_file(source.path().join("memory/2026-03-10.md"), "day two");
    write_file(source.path().join("SOUL.md"), "soul");
    write_file(source.path().join("CUSTOM.md"), "custom");

    let report = execute_import(&test_options(source.path(), data.path())).expect("import");
    let output = render_import_report(&report);

    assert!(output.contains("  Memory:"));
    assert!(output.contains("  Context:"));
    assert!(
        output.contains("Imported 4 files (2 memory, 2 context). Your memory and context are ready.")
    );
    assert!(output.contains("Fawx loads all .md files from ~/.fawx/context/ automatically."));
}

fn test_options(source_dir: &Path, data_dir: &Path) -> ImportOptions {
    ImportOptions {
        source_dir: source_dir.to_path_buf(),
        data_dir: data_dir.to_path_buf(),
        dry_run: false,
        force: false,
    }
}

fn seed_workspace_markdown_files(source_dir: &Path) {
    write_file(source_dir.join("MEMORY.md"), "long memory");
    write_file(source_dir.join("SOUL.md"), "soul");
    write_file(source_dir.join("USER.md"), "user");
    write_file(source_dir.join("AGENTS.md"), "agents");
    write_file(source_dir.join("IDENTITY.md"), "identity");
    write_file(source_dir.join("TOOLS.md"), "tools");
    write_file(source_dir.join("ENGINEERING.md"), "engineering");
    write_file(source_dir.join("CUSTOM.md"), "custom");
    write_file(source_dir.join("memory/2026-03-09.md"), "day one");
    write_file(source_dir.join("memory/2026-03-10.md"), "day two");
}

fn write_file(path: PathBuf, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, body).expect("write file");
}

fn read(path: PathBuf) -> String {
    fs::read_to_string(path).expect("read file")
}

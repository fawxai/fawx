//! Skill management commands.

use anyhow::{Context, Result};
use fx_author::{BuildConfig, BuildResult};
use fx_skills::manifest::{
    validate_skill_name as validate_manifest_skill_name, Capability, ALL_CAPABILITIES,
};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_LEN: usize = 1024;
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;
const MAX_CAPABILITIES: usize = 10;
/// Get the skills directory path.
fn get_skills_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let skills_dir = home.join(".fawx").join("skills");
    fs::create_dir_all(&skills_dir)
        .with_context(|| format!("Failed to create skills directory: {:?}", skills_dir))?;
    Ok(skills_dir)
}

/// Install a skill from a WASM file and manifest.
pub async fn install(path: &str) -> Result<()> {
    let input_path = Path::new(path);
    ensure_input_exists(path, input_path)?;

    let (wasm_path, manifest_path) = resolve_skill_paths(input_path)?;
    let manifest = load_manifest(&manifest_path)?;
    validate_manifest_fields(&manifest)?;

    let wasm_bytes = fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {:?}", wasm_path))?;
    validate_wasm(&manifest, &wasm_bytes)?;

    install_skill_files(&manifest, &wasm_path, &manifest_path)?;
    Ok(())
}

fn ensure_input_exists(path: &str, input_path: &Path) -> Result<()> {
    if input_path.exists() {
        return Ok(());
    }
    anyhow::bail!("File not found: {}", path);
}

fn resolve_skill_paths(input_path: &Path) -> Result<(PathBuf, PathBuf)> {
    if input_path.is_dir() {
        return resolve_paths_from_directory(input_path);
    }

    if input_path.extension().and_then(|e| e.to_str()) == Some("wasm") {
        return resolve_paths_from_wasm(input_path);
    }

    anyhow::bail!("Expected a .wasm file or skill directory");
}

fn resolve_paths_from_directory(input_path: &Path) -> Result<(PathBuf, PathBuf)> {
    let dir_name = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid directory name")?;

    let wasm_name = format!("{}.wasm", dir_name.replace("-skill", ""));
    let wasm = input_path.join(&wasm_name);
    let manifest = input_path.join("manifest.toml");

    ensure_file_exists(&wasm, "WASM file")?;
    ensure_file_exists(&manifest, "Manifest")?;
    Ok((wasm, manifest))
}

fn resolve_paths_from_wasm(input_path: &Path) -> Result<(PathBuf, PathBuf)> {
    let wasm = input_path.to_path_buf();
    let manifest = input_path.with_extension("toml");

    if manifest.exists() {
        return Ok((wasm, manifest));
    }

    let parent = input_path.parent().context("No parent directory")?;
    let manifest_alt = parent.join("manifest.toml");
    if manifest_alt.exists() {
        return Ok((wasm, manifest_alt));
    }

    anyhow::bail!(
        "Manifest not found. Expected at {:?} or {:?}",
        manifest,
        manifest_alt
    );
}

fn ensure_file_exists(path: &Path, label: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    anyhow::bail!("{} not found: {:?}", label, path);
}

fn load_manifest(manifest_path: &Path) -> Result<fx_skills::manifest::SkillManifest> {
    let manifest_content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read manifest: {:?}", manifest_path))?;

    let manifest: fx_skills::manifest::SkillManifest =
        toml::from_str(&manifest_content).context("Failed to parse manifest")?;

    fx_skills::manifest::validate_manifest(&manifest).context("Manifest validation failed")?;
    Ok(manifest)
}

fn validate_manifest_fields(manifest: &fx_skills::manifest::SkillManifest) -> Result<()> {
    validate_manifest_name_length(&manifest.name)?;
    validate_manifest_skill_name(&manifest.name)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    if manifest.description.len() > MAX_DESCRIPTION_LEN {
        anyhow::bail!(
            "Skill description too long (max {} chars)",
            MAX_DESCRIPTION_LEN
        );
    }

    if manifest.capabilities.len() > MAX_CAPABILITIES {
        anyhow::bail!("Too many capabilities (max {})", MAX_CAPABILITIES);
    }

    Ok(())
}

fn has_invalid_skill_name(name: &str) -> bool {
    name.contains("..") || name.contains('/') || name.contains('\\')
}

fn validate_wasm(manifest: &fx_skills::manifest::SkillManifest, wasm_bytes: &[u8]) -> Result<()> {
    if wasm_bytes.len() > MAX_WASM_SIZE {
        anyhow::bail!(
            "WASM file too large: {} bytes (max {} MB)",
            wasm_bytes.len(),
            MAX_WASM_SIZE / (1024 * 1024)
        );
    }

    let loader = fx_skills::loader::SkillLoader::new(vec![]);
    loader
        .load(wasm_bytes, manifest, None)
        .context("Failed to load/validate WASM module")?;
    Ok(())
}

fn install_skill_files(
    manifest: &fx_skills::manifest::SkillManifest,
    wasm_path: &Path,
    manifest_path: &Path,
) -> Result<()> {
    let skills_dir = get_skills_dir()?;
    let skill_dir = skills_dir.join(&manifest.name);
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("Failed to create skill directory: {:?}", skill_dir))?;

    let dest_wasm = skill_dir.join(format!("{}.wasm", manifest.name));
    fs::copy(wasm_path, &dest_wasm)
        .with_context(|| format!("Failed to copy WASM to {:?}", dest_wasm))?;

    let dest_manifest = skill_dir.join("manifest.toml");
    fs::copy(manifest_path, &dest_manifest)
        .with_context(|| format!("Failed to copy manifest to {:?}", dest_manifest))?;

    print_install_summary(manifest, &skill_dir);
    Ok(())
}

fn print_install_summary(manifest: &fx_skills::manifest::SkillManifest, skill_dir: &Path) {
    println!(
        "✓ Installed skill '{}' v{}",
        manifest.name, manifest.version
    );
    println!("  Location: {:?}", skill_dir);

    if !manifest.capabilities.is_empty() {
        println!(
            "  Capabilities: {}",
            format_capabilities(&manifest.capabilities)
        );
    }
}

/// List installed skills.
pub async fn list() -> Result<()> {
    let skills_dir = get_skills_dir()?;
    let entries = list_skill_directories(&skills_dir)?;

    if entries.is_empty() {
        print_empty_skills_message();
        return Ok(());
    }

    println!("Installed skills:\n");
    for entry in entries {
        print_skill_entry(&entry.path());
    }

    Ok(())
}

fn list_skill_directories(skills_dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let entries = fs::read_dir(skills_dir)
        .context("Failed to read skills directory")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();
    Ok(entries)
}

fn print_empty_skills_message() {
    println!("No skills installed.");
    println!();
    println!("To install a skill:");
    println!("  fawx skill install <path-to-skill>");
}

fn print_skill_entry(skill_dir: &Path) {
    let manifest_path = skill_dir.join("manifest.toml");
    if !manifest_path.exists() {
        eprintln!("⚠ Skipping {:?}: manifest.toml not found", skill_dir);
        return;
    }

    match read_manifest_for_listing(&manifest_path) {
        Ok(manifest) => print_manifest_for_listing(&manifest),
        Err(error) => eprintln!("⚠ {}", error),
    }
}

fn read_manifest_for_listing(
    manifest_path: &Path,
) -> Result<fx_skills::manifest::SkillManifest, String> {
    let manifest_content = fs::read_to_string(manifest_path)
        .map_err(|error| format!("Failed to read manifest at {:?}: {}", manifest_path, error))?;

    toml::from_str(&manifest_content)
        .map_err(|error| format!("Failed to parse manifest at {:?}: {}", manifest_path, error))
}

fn print_manifest_for_listing(manifest: &fx_skills::manifest::SkillManifest) {
    println!("  {} v{}", manifest.name, manifest.version);
    println!("    {}", manifest.description);

    if !manifest.capabilities.is_empty() {
        println!(
            "    Capabilities: {}",
            format_capabilities(&manifest.capabilities)
        );
    }

    println!();
}

fn format_capabilities(capabilities: &[Capability]) -> String {
    capabilities
        .iter()
        .map(|capability| capability.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Remove an installed skill.
pub async fn remove(name: &str) -> Result<()> {
    if has_invalid_skill_name(name) {
        anyhow::bail!("Invalid skill name: must not contain path separators or '..'");
    }

    let skills_dir = get_skills_dir()?;
    let skill_dir = skills_dir.join(name);

    if !skill_dir.exists() {
        anyhow::bail!("Skill '{}' is not installed", name);
    }

    fs::remove_dir_all(&skill_dir)
        .with_context(|| format!("Failed to remove skill directory: {:?}", skill_dir))?;

    println!("✓ Removed skill '{}'", name);
    Ok(())
}

/// Build a skill from source.
pub fn build(path: &str, no_sign: bool, no_install: bool) -> Result<()> {
    let project_path = PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Invalid project path: {path}"))?;

    let data_dir = resolve_data_dir()?;

    let config = BuildConfig {
        project_path,
        data_dir,
        no_sign,
        no_install,
    };

    let result = fx_author::build_skill(&config).map_err(|e| anyhow::anyhow!("{e}"))?;
    print_build_summary(&result);
    Ok(())
}

/// Scaffold a new skill project.
pub fn create(
    name: &str,
    capabilities: Option<&str>,
    tool_name: Option<&str>,
    path: Option<&str>,
) -> Result<()> {
    let options = CreateOptions::new(name, capabilities, tool_name, path)?;
    let project_dir = scaffold_skill_project(&options)?;
    print_create_summary(&project_dir, &options.name);
    Ok(())
}

#[derive(Debug)]
struct CreateOptions {
    name: String,
    tool_name: String,
    capabilities: Vec<Capability>,
    parent_dir: PathBuf,
}

impl CreateOptions {
    fn new(
        name: &str,
        capabilities: Option<&str>,
        tool_name: Option<&str>,
        path: Option<&str>,
    ) -> Result<Self> {
        validate_skill_name(name)?;
        let parent_dir = resolve_parent_dir(path)?;
        let capabilities = parse_capabilities(capabilities)?;
        let tool_name = tool_name.unwrap_or(name).to_string();
        Ok(Self {
            name: name.to_string(),
            tool_name,
            capabilities,
            parent_dir,
        })
    }
}

fn resolve_parent_dir(path: Option<&str>) -> Result<PathBuf> {
    match path {
        Some(path) => Ok(PathBuf::from(path)),
        None => {
            let cwd = std::env::current_dir().context("Failed to get current directory")?;
            Ok(cwd.join("skills"))
        }
    }
}

fn validate_skill_name(name: &str) -> Result<()> {
    validate_manifest_name_length(name)?;
    validate_manifest_skill_name(name).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        anyhow::bail!("name must contain only alphanumeric characters and hyphens");
    }
    Ok(())
}

fn validate_manifest_name_length(name: &str) -> Result<()> {
    if name.len() > MAX_NAME_LEN {
        anyhow::bail!("name must be {} characters or fewer", MAX_NAME_LEN);
    }
    Ok(())
}

fn parse_capabilities(input: Option<&str>) -> Result<Vec<Capability>> {
    input
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|capability| !capability.is_empty())
                .map(parse_capability)
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn parse_capability(capability: &str) -> Result<Capability> {
    Capability::parse(capability).ok_or_else(|| {
        let valid = ALL_CAPABILITIES
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::anyhow!("unknown capability '{}', valid: {}", capability, valid)
    })
}

fn scaffold_skill_project(options: &CreateOptions) -> Result<PathBuf> {
    let project_dir = options.parent_dir.join(&options.name);
    ensure_project_dir_absent(&project_dir)?;
    fs::create_dir_all(project_dir.join("src"))
        .with_context(|| format!("Failed to create directory: {}", project_dir.display()))?;
    write_scaffold_files(&project_dir, options)?;
    Ok(project_dir)
}

fn ensure_project_dir_absent(project_dir: &Path) -> Result<()> {
    if project_dir.exists() {
        anyhow::bail!("directory already exists: {}", project_dir.display());
    }
    Ok(())
}

fn write_scaffold_files(project_dir: &Path, options: &CreateOptions) -> Result<()> {
    write_file(&project_dir.join("Cargo.toml"), &cargo_toml(&options.name))?;
    write_file(
        &project_dir.join("manifest.toml"),
        &manifest_toml(&options.name, &options.tool_name, &options.capabilities),
    )?;
    write_file(&project_dir.join("src/lib.rs"), &lib_rs(&options.name))?;
    write_file(&project_dir.join(".gitignore"), "/target\n")?;
    write_file(&project_dir.join("README.md"), &readme_md(&options.name))?;
    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content).with_context(|| format!("Failed to write file: {}", path.display()))
}

fn cargo_toml(name: &str) -> String {
    format!(
        concat!(
            "[package]\n",
            "name = \"{name}\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2021\"\n\n",
            "[lib]\n",
            "crate-type = [\"cdylib\"]\n\n",
            "[dependencies]\n",
            "# No deps by default — host API is provided via imports\n"
        ),
        name = name
    )
}

fn manifest_toml(name: &str, tool_name: &str, capabilities: &[Capability]) -> String {
    let capabilities = manifest_capabilities(capabilities);
    format!(
        concat!(
            "name = \"{name}\"\n",
            "version = \"0.1.0\"\n",
            "description = \"A Fawx skill\"\n",
            "author = \"TODO: set author\"\n",
            "api_version = \"host_api_v2\"\n",
            "entry_point = \"run\"\n",
            "capabilities = [{capabilities}]\n\n",
            "[[tools]]\n",
            "name = \"{tool_name}\"\n",
            "description = \"TODO: describe what this tool does\"\n\n",
            "[[tools.parameters]]\n",
            "name = \"input\"\n",
            "type = \"string\"\n",
            "description = \"TODO: describe the input parameter\"\n",
            "required = true\n"
        ),
        name = name,
        capabilities = capabilities,
        tool_name = tool_name
    )
}

fn manifest_capabilities(capabilities: &[Capability]) -> String {
    capabilities
        .iter()
        .map(|capability| format!("\"{}\"", capability))
        .collect::<Vec<_>>()
        .join(", ")
}

fn lib_rs(name: &str) -> String {
    format!(
        concat!(
            "//! {name} — a Fawx WASM skill.\n\n",
            "/// Entry point called by the Fawx host.\n",
            "///\n",
            "/// The host provides input as a JSON string via the `input` parameter.\n",
            "/// Return a JSON string as the tool result.\n",
            "#[no_mangle]\n",
            "pub extern \"C\" fn run(input_ptr: *const u8, input_len: usize) -> u64 {{\n",
            "    let input = unsafe {{\n",
            "        let slice = std::slice::from_raw_parts(input_ptr, input_len);\n",
            "        std::str::from_utf8_unchecked(slice)\n",
            "    }};\n\n",
            "    // TODO: implement your skill logic here\n",
            "    let result = format!(\"{{{{\\\"result\\\": \\\"Hello from {name}! Input was: {{}}\\\"}}}}\", input);\n\n",
            "    let bytes = result.into_bytes();\n",
            "    let ptr = bytes.as_ptr() as u64;\n",
            "    let len = bytes.len() as u64;\n",
            "    std::mem::forget(bytes);\n\n",
            "    (ptr << 32) | len\n",
            "}}\n"
        ),
        name = name
    )
}

fn readme_md(name: &str) -> String {
    format!(
        concat!(
            "# {name}\n\n",
            "A Fawx WASM skill.\n\n",
            "## Build\n\n",
            "```bash\n",
            "cargo build --release --target wasm32-unknown-unknown\n",
            "```\n\n",
            "## Install\n\n",
            "```bash\n",
            "fawx skill install target/wasm32-unknown-unknown/release/{name}.wasm\n",
            "```\n"
        ),
        name = name
    )
}

fn print_create_summary(project_dir: &Path, name: &str) {
    println!("Created skill project: {}/", project_dir.display());
    println!();
    println!("To build:");
    println!("  cd {}", project_dir.display());
    println!("  cargo build --release --target wasm32-unknown-unknown");
    println!();
    println!("To install:");
    println!(
        "  fawx skill install target/wasm32-unknown-unknown/release/{}.wasm",
        name.replace('-', "_")
    );
}

fn resolve_data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".fawx"))
}

fn print_build_summary(result: &BuildResult) {
    let size_kb = result.wasm_size_bytes / 1024;
    let signed_str = if result.signed { "signed" } else { "unsigned" };

    println!(
        "✓ Built {} v{} ({} KB, {})",
        result.skill_name, result.version, size_kb, signed_str
    );

    if let Some(ref path) = result.install_path {
        println!("  Installed to: {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_skills::manifest::{parse_manifest, validate_manifest};
    use tempfile::TempDir;

    fn parse_generated_lib_rs(name: &str) -> syn::File {
        syn::parse_file(&lib_rs(name)).expect("generated lib.rs should parse as Rust")
    }

    #[test]
    fn create_scaffolds_all_files() {
        let temp_dir = TempDir::new().expect("temp dir");
        let options =
            CreateOptions::new("weather-skill", None, None, Some(path_str(temp_dir.path())))
                .expect("options");

        let project_dir = scaffold_skill_project(&options).expect("scaffold project");

        assert_eq!(project_dir, temp_dir.path().join("weather-skill"));
        assert_eq!(
            read(project_dir.join("Cargo.toml")),
            cargo_toml("weather-skill")
        );
        assert_eq!(
            read(project_dir.join("manifest.toml")),
            manifest_toml("weather-skill", "weather-skill", &[])
        );
        assert_eq!(
            read(project_dir.join("src/lib.rs")),
            lib_rs("weather-skill")
        );
        assert_eq!(read(project_dir.join(".gitignore")), "/target\n");
        assert_eq!(
            read(project_dir.join("README.md")),
            readme_md("weather-skill")
        );
    }

    #[test]
    fn create_with_capabilities() {
        let temp_dir = TempDir::new().expect("temp dir");
        let options = CreateOptions::new(
            "weather-skill",
            Some("network,storage"),
            None,
            Some(path_str(temp_dir.path())),
        )
        .expect("options");

        let project_dir = scaffold_skill_project(&options).expect("scaffold project");
        let manifest = read(project_dir.join("manifest.toml"));

        assert!(manifest.contains("capabilities = [\"network\", \"storage\"]"));
    }

    #[test]
    fn create_with_custom_tool_name() {
        let temp_dir = TempDir::new().expect("temp dir");
        let options = CreateOptions::new(
            "weather-skill",
            None,
            Some("my_tool"),
            Some(path_str(temp_dir.path())),
        )
        .expect("options");

        let project_dir = scaffold_skill_project(&options).expect("scaffold project");
        let manifest = read(project_dir.join("manifest.toml"));

        assert!(manifest.contains("name = \"my_tool\""));
    }

    #[test]
    fn create_with_custom_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let custom_root = temp_dir.path().join("test-skills");
        let options = CreateOptions::new("weather-skill", None, None, Some(path_str(&custom_root)))
            .expect("options");

        let project_dir = scaffold_skill_project(&options).expect("scaffold project");

        assert_eq!(project_dir, custom_root.join("weather-skill"));
        assert!(project_dir.join("Cargo.toml").exists());
    }

    #[test]
    fn create_rejects_invalid_name() {
        assert_invalid_name("../evil");
        assert_invalid_name("foo/bar");
        assert_invalid_name("");
        assert_invalid_name(&"a".repeat(65));
    }

    #[test]
    fn create_rejects_existing_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let project_dir = temp_dir.path().join("weather-skill");
        fs::create_dir_all(&project_dir).expect("create dir");
        let options =
            CreateOptions::new("weather-skill", None, None, Some(path_str(temp_dir.path())))
                .expect("options");

        let error = scaffold_skill_project(&options).expect_err("existing directory should fail");

        assert_eq!(
            error.to_string(),
            format!("directory already exists: {}", project_dir.display())
        );
    }

    #[test]
    fn create_rejects_unknown_capability() {
        let error = CreateOptions::new("weather-skill", Some("flying"), None, None)
            .expect_err("unknown capability should fail");

        assert_eq!(
            error.to_string(),
            "unknown capability 'flying', valid: network, storage, shell, filesystem, notifications, sensors, phone_actions"
        );
    }

    #[test]
    fn generated_lib_rs_parses_as_rust() {
        let parsed = parse_generated_lib_rs("weather-skill");

        assert_eq!(parsed.items.len(), 1);
    }

    #[test]
    fn generated_lib_rs_keeps_inner_format_braces_escaped() {
        let generated = lib_rs("weather-skill");

        assert!(generated.contains("format!(\"{{\\\"result\\\":"));
        assert!(generated.contains("Input was: {}"));
        assert!(generated.contains("\\\"}}\", input);"));
    }

    #[test]
    fn create_manifest_parses_cleanly() {
        let temp_dir = TempDir::new().expect("temp dir");
        let options = CreateOptions::new(
            "weather-skill",
            Some("network,storage"),
            Some("weather_tool"),
            Some(path_str(temp_dir.path())),
        )
        .expect("options");

        let project_dir = scaffold_skill_project(&options).expect("scaffold project");
        let manifest = read(project_dir.join("manifest.toml"));
        let parsed = parse_manifest(&manifest).expect("manifest should parse");

        validate_manifest(&parsed).expect("manifest should validate");
        assert_eq!(parsed.author, "TODO: set author");
        assert_eq!(parsed.name, "weather-skill");
        assert_eq!(parsed.api_version, "host_api_v2");
        assert_eq!(
            parsed.capabilities,
            vec![Capability::Network, Capability::Storage]
        );
    }

    fn assert_invalid_name(name: &str) {
        assert!(CreateOptions::new(name, None, None, None).is_err());
    }

    fn read(path: PathBuf) -> String {
        fs::read_to_string(path).expect("read file")
    }

    fn path_str(path: &Path) -> &str {
        path.to_str().expect("utf-8 path")
    }
}

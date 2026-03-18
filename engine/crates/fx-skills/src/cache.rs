//! Module caching for faster skill loading.
//!
//! Uses safe `Module::new()` compilation only. We intentionally avoid
//! `Module::deserialize()` (unsafe) because it loads pre-compiled native
//! code directly into executable memory — an attacker who can modify
//! cached files could achieve arbitrary code execution.
//!
//! The cache tracks which WASM bytes have been seen (via SHA-256 hash)
//! for statistics and cache management. Actual compilation always goes
//! through the safe `Module::new()` path.

use fx_core::error::SkillError;
use ring::digest::{digest, SHA256};
use std::fs;
use std::path::PathBuf;
use wasmtime::{Engine, Module};

/// Get the cache directory path.
fn get_cache_dir() -> Result<PathBuf, SkillError> {
    let home = dirs::home_dir()
        .ok_or_else(|| SkillError::Load("Failed to get home directory".to_string()))?;

    let cache_dir = home.join(".fawx").join("cache").join("skills");

    // Create directory if it doesn't exist
    fs::create_dir_all(&cache_dir)
        .map_err(|e| SkillError::Load(format!("Failed to create cache directory: {}", e)))?;

    Ok(cache_dir)
}

/// Compute SHA-256 hash of bytes.
fn hash_bytes(data: &[u8]) -> String {
    let hash = digest(&SHA256, data);
    hex::encode(hash.as_ref())
}

/// Safely compile a WASM module, using the cache for deduplication tracking.
///
/// Always compiles from source WASM bytes via `Module::new()` (safe).
/// Records the WASM hash in the cache directory for statistics.
///
/// Returns `(Module, bool)` where the bool indicates if this WASM was seen before.
pub fn compile_module(engine: &Engine, wasm_bytes: &[u8]) -> Result<(Module, bool), SkillError> {
    let cache_dir = get_cache_dir()?;
    compile_module_in(engine, wasm_bytes, &cache_dir)
}

/// Compile a WASM module using an explicit cache directory.
pub fn compile_module_in(
    engine: &Engine,
    wasm_bytes: &[u8],
    cache_dir: &std::path::Path,
) -> Result<(Module, bool), SkillError> {
    fs::create_dir_all(cache_dir)
        .map_err(|e| SkillError::Load(format!("Failed to create cache dir: {e}")))?;
    let hash = hash_bytes(wasm_bytes);
    let cache_path = cache_dir.join(format!("{hash}.module"));
    let was_cached = cache_path.exists();

    let module = Module::new(engine, wasm_bytes)
        .map_err(|e| SkillError::Load(format!("Failed to compile WASM module: {e}")))?;

    if !was_cached {
        let _ = fs::write(&cache_path, hash.as_bytes());
    }

    Ok((module, was_cached))
}

/// Check if WASM bytes have been compiled before.
pub fn has_cached_module(wasm_bytes: &[u8]) -> Result<bool, SkillError> {
    let cache_dir = get_cache_dir()?;
    has_cached_module_in(wasm_bytes, &cache_dir)
}

/// Check if WASM bytes have been compiled before, using an explicit cache dir.
pub fn has_cached_module_in(
    wasm_bytes: &[u8],
    cache_dir: &std::path::Path,
) -> Result<bool, SkillError> {
    let hash = hash_bytes(wasm_bytes);
    Ok(cache_dir.join(format!("{hash}.module")).exists())
}

/// Clear the entire module cache.
pub fn clear_cache() -> Result<(), SkillError> {
    let cache_dir = get_cache_dir()?;
    clear_cache_in(&cache_dir)
}

/// Clear the module cache in an explicit directory.
pub fn clear_cache_in(cache_dir: &std::path::Path) -> Result<(), SkillError> {
    if !cache_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(cache_dir)
        .map_err(|e| SkillError::Load(format!("Failed to read cache directory: {e}")))?
    {
        let entry =
            entry.map_err(|e| SkillError::Load(format!("Failed to read cache entry: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("module") {
            fs::remove_file(&path)
                .map_err(|e| SkillError::Load(format!("Failed to remove cache file: {e}")))?;
        }
    }
    Ok(())
}

/// Get cache statistics.
pub fn cache_stats() -> Result<CacheStats, SkillError> {
    let cache_dir = get_cache_dir()?;
    cache_stats_in(&cache_dir)
}

/// Get cache statistics for an explicit directory.
pub fn cache_stats_in(cache_dir: &std::path::Path) -> Result<CacheStats, SkillError> {
    if !cache_dir.exists() {
        return Ok(CacheStats {
            num_entries: 0,
            total_size: 0,
        });
    }
    let mut num_entries = 0;
    let mut total_size = 0u64;
    for entry in fs::read_dir(cache_dir)
        .map_err(|e| SkillError::Load(format!("Failed to read cache directory: {e}")))?
    {
        let entry =
            entry.map_err(|e| SkillError::Load(format!("Failed to read cache entry: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("module") {
            num_entries += 1;
            if let Ok(metadata) = fs::metadata(&path) {
                total_size += metadata.len();
            }
        }
    }
    Ok(CacheStats {
        num_entries,
        total_size,
    })
}

/// Cache statistics.
#[derive(Debug)]
pub struct CacheStats {
    /// Number of cached modules
    pub num_entries: usize,
    /// Total size in bytes
    pub total_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use wasmtime::Engine;

    fn create_minimal_wasm() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6d, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
        ]
    }

    #[test]
    fn test_hash_bytes() {
        let data = b"test data";
        let hash1 = hash_bytes(data);
        let hash2 = hash_bytes(data);
        assert_eq!(hash1, hash2);

        let other_data = b"other data";
        let hash3 = hash_bytes(other_data);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_compile_new_module() {
        let tmp = TempDir::new().expect("tempdir");
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        let (module, was_cached) = compile_module_in(&engine, &wasm, tmp.path()).expect("compile");
        assert!(!was_cached);
        assert_eq!(module.exports().count(), 0);
    }

    #[test]
    fn test_compile_cached_module() {
        let tmp = TempDir::new().expect("tempdir");
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        let (_m1, cached1) = compile_module_in(&engine, &wasm, tmp.path()).expect("first compile");
        assert!(!cached1);

        let (_m2, cached2) = compile_module_in(&engine, &wasm, tmp.path()).expect("second compile");
        assert!(cached2);
    }

    #[test]
    fn test_has_cached_module() {
        let tmp = TempDir::new().expect("tempdir");
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        assert!(!has_cached_module_in(&wasm, tmp.path()).expect("check before"));

        compile_module_in(&engine, &wasm, tmp.path()).expect("compile");

        assert!(has_cached_module_in(&wasm, tmp.path()).expect("check after"));
    }

    #[test]
    fn test_different_wasm_not_cached() {
        let tmp = TempDir::new().expect("tempdir");
        let engine = Engine::default();
        let wasm1 = create_minimal_wasm();
        let wasm2 = b"different content with different hash";

        compile_module_in(&engine, &wasm1, tmp.path()).expect("compile wasm1");

        assert!(!has_cached_module_in(wasm2, tmp.path()).expect("check wasm2"));
    }

    #[test]
    fn test_cache_stats() {
        let tmp = TempDir::new().expect("tempdir");
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        let before = cache_stats_in(tmp.path()).expect("stats before");
        assert_eq!(before.num_entries, 0);

        compile_module_in(&engine, &wasm, tmp.path()).expect("compile");

        let after = cache_stats_in(tmp.path()).expect("stats after");
        assert_eq!(after.num_entries, 1);
    }

    #[test]
    fn test_clear_cache() {
        let tmp = TempDir::new().expect("tempdir");
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        compile_module_in(&engine, &wasm, tmp.path()).expect("compile");
        let stats = cache_stats_in(tmp.path()).expect("stats");
        assert!(stats.num_entries > 0);

        clear_cache_in(tmp.path()).expect("clear");
        let stats = cache_stats_in(tmp.path()).expect("stats after clear");
        assert_eq!(stats.num_entries, 0);
    }
}

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

/// Get the cache marker file path for a given WASM hash.
fn get_cache_path(hash: &str) -> Result<PathBuf, SkillError> {
    let cache_dir = get_cache_dir()?;
    Ok(cache_dir.join(format!("{}.module", hash)))
}

/// Safely compile a WASM module, using the cache for deduplication tracking.
///
/// Always compiles from source WASM bytes via `Module::new()` (safe).
/// Records the WASM hash in the cache directory for statistics.
///
/// Returns `(Module, bool)` where the bool indicates if this WASM was seen before.
pub fn compile_module(engine: &Engine, wasm_bytes: &[u8]) -> Result<(Module, bool), SkillError> {
    let hash = hash_bytes(wasm_bytes);
    let cache_path = get_cache_path(&hash)?;
    let was_cached = cache_path.exists();

    // Always compile safely from source
    let module = Module::new(engine, wasm_bytes)
        .map_err(|e| SkillError::Load(format!("Failed to compile WASM module: {}", e)))?;

    // Write cache marker (just the hash, not native code)
    if !was_cached {
        let _ = fs::write(&cache_path, hash.as_bytes());
        tracing::debug!("Compiled and cached new module with hash {}", hash);
    } else {
        tracing::debug!("Recompiled known module with hash {}", hash);
    }

    Ok((module, was_cached))
}

/// Check if WASM bytes have been compiled before.
pub fn has_cached_module(wasm_bytes: &[u8]) -> Result<bool, SkillError> {
    let hash = hash_bytes(wasm_bytes);
    let cache_path = get_cache_path(&hash)?;
    Ok(cache_path.exists())
}

/// Clear the entire module cache.
pub fn clear_cache() -> Result<(), SkillError> {
    let cache_dir = get_cache_dir()?;

    if !cache_dir.exists() {
        return Ok(());
    }

    // Remove all .module files
    for entry in fs::read_dir(&cache_dir)
        .map_err(|e| SkillError::Load(format!("Failed to read cache directory: {}", e)))?
    {
        let entry =
            entry.map_err(|e| SkillError::Load(format!("Failed to read cache entry: {}", e)))?;

        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("module") {
            fs::remove_file(&path)
                .map_err(|e| SkillError::Load(format!("Failed to remove cache file: {}", e)))?;
        }
    }

    tracing::info!("Cleared module cache");
    Ok(())
}

/// Get cache statistics.
pub fn cache_stats() -> Result<CacheStats, SkillError> {
    let cache_dir = get_cache_dir()?;

    if !cache_dir.exists() {
        return Ok(CacheStats {
            num_entries: 0,
            total_size: 0,
        });
    }

    let mut num_entries = 0;
    let mut total_size = 0u64;

    for entry in fs::read_dir(&cache_dir)
        .map_err(|e| SkillError::Load(format!("Failed to read cache directory: {}", e)))?
    {
        let entry =
            entry.map_err(|e| SkillError::Load(format!("Failed to read cache entry: {}", e)))?;

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
    use std::sync::Mutex;
    use wasmtime::Engine;

    // Serialize cache tests to prevent races on the shared cache directory
    static CACHE_LOCK: Mutex<()> = Mutex::new(());

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
        let _lock = CACHE_LOCK.lock().unwrap();
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        // Clear cache first
        clear_cache().ok();

        let (module, was_cached) = compile_module(&engine, &wasm).expect("Should compile");
        assert!(!was_cached);
        assert_eq!(module.exports().count(), 0); // Minimal WASM has no exports
    }

    #[test]
    fn test_compile_cached_module() {
        let _lock = CACHE_LOCK.lock().unwrap();
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        // Clear cache first
        clear_cache().ok();

        // First compile
        let (_module1, was_cached1) = compile_module(&engine, &wasm).expect("Should compile");
        assert!(!was_cached1);

        // Second compile — should report as previously seen
        let (_module2, was_cached2) = compile_module(&engine, &wasm).expect("Should compile");
        assert!(was_cached2);
    }

    #[test]
    fn test_has_cached_module() {
        let _lock = CACHE_LOCK.lock().unwrap();
        let wasm = create_minimal_wasm();
        let engine = Engine::default();

        // Clear cache first
        clear_cache().ok();

        assert!(!has_cached_module(&wasm).expect("Should check"));

        compile_module(&engine, &wasm).expect("Should compile");

        assert!(has_cached_module(&wasm).expect("Should check"));
    }

    #[test]
    fn test_different_wasm_not_cached() {
        let _lock = CACHE_LOCK.lock().unwrap();
        let engine = Engine::default();
        let wasm1 = create_minimal_wasm();

        // Different bytes produce a different hash
        let wasm2 = b"different content that won't compile but has a different hash";

        // Clear cache first
        clear_cache().ok();

        compile_module(&engine, &wasm1).expect("Should compile wasm1");

        // Different bytes should not show as cached
        assert!(!has_cached_module(wasm2).expect("Should check"));
    }

    #[test]
    fn test_cache_stats() {
        let _lock = CACHE_LOCK.lock().unwrap();
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        // Clear cache first
        clear_cache().ok();

        let stats_before = cache_stats().expect("Should get stats");
        assert_eq!(stats_before.num_entries, 0);

        compile_module(&engine, &wasm).expect("Should compile");

        let stats_after = cache_stats().expect("Should get stats");
        assert_eq!(stats_after.num_entries, 1);
    }

    #[test]
    fn test_clear_cache() {
        let _lock = CACHE_LOCK.lock().unwrap();
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        compile_module(&engine, &wasm).expect("Should compile");

        let stats = cache_stats().expect("Should get stats");
        assert!(stats.num_entries > 0);

        clear_cache().expect("Should clear");

        let stats = cache_stats().expect("Should get stats");
        assert_eq!(stats.num_entries, 0);
    }
}

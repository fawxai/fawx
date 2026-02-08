//! Module caching for faster skill loading.

use nv_core::error::SkillError;
use ring::digest::{digest, SHA256};
use std::fs;
use std::path::PathBuf;
use wasmtime::{Engine, Module};

/// Get the cache directory path.
fn get_cache_dir() -> Result<PathBuf, SkillError> {
    let home = dirs::home_dir()
        .ok_or_else(|| SkillError::Load("Failed to get home directory".to_string()))?;

    let cache_dir = home.join(".nova").join("cache").join("skills");

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

/// Get the cache file path for a given WASM hash.
fn get_cache_path(hash: &str) -> Result<PathBuf, SkillError> {
    let cache_dir = get_cache_dir()?;
    Ok(cache_dir.join(format!("{}.module", hash)))
}

/// Try to load a cached module.
///
/// Returns `Some(Module)` if cache hit, `None` if cache miss.
pub fn load_cached_module(
    engine: &Engine,
    wasm_bytes: &[u8],
) -> Result<Option<Module>, SkillError> {
    let hash = hash_bytes(wasm_bytes);
    let cache_path = get_cache_path(&hash)?;

    if !cache_path.exists() {
        tracing::debug!("Cache miss for hash {}", hash);
        return Ok(None);
    }

    // Read cached bytes
    let cached_bytes = fs::read(&cache_path)
        .map_err(|e| SkillError::Load(format!("Failed to read cached module: {}", e)))?;

    // Deserialize module
    // Safety: We're trusting the cache. In a production system, you might want
    // additional validation (e.g., checking a signature file).
    let module = unsafe { Module::deserialize(engine, &cached_bytes) }.map_err(|e| {
        // If deserialization fails, remove the corrupt cache file
        let _ = fs::remove_file(&cache_path);
        SkillError::Load(format!("Failed to deserialize cached module: {}", e))
    })?;

    tracing::debug!("Cache hit for hash {}", hash);
    Ok(Some(module))
}

/// Cache a compiled module.
pub fn cache_module(wasm_bytes: &[u8], module: &Module) -> Result<(), SkillError> {
    let hash = hash_bytes(wasm_bytes);
    let cache_path = get_cache_path(&hash)?;

    // Serialize module
    let serialized = module
        .serialize()
        .map_err(|e| SkillError::Load(format!("Failed to serialize module: {}", e)))?;

    // Write to cache
    fs::write(&cache_path, serialized)
        .map_err(|e| SkillError::Load(format!("Failed to write cache: {}", e)))?;

    tracing::debug!("Cached module with hash {}", hash);
    Ok(())
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
    fn test_cache_miss() {
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        // Clear cache first
        clear_cache().ok();

        let result = load_cached_module(&engine, &wasm).expect("Should not error");
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_hit() {
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        // Clear cache first
        clear_cache().ok();

        // Compile and cache
        let module = Module::new(&engine, &wasm).expect("Should compile");
        cache_module(&wasm, &module).expect("Should cache");

        // Load from cache
        let cached = load_cached_module(&engine, &wasm)
            .expect("Should not error")
            .expect("Should be cached");

        // Both modules should work
        assert!(module.exports().count() == cached.exports().count());
    }

    #[test]
    fn test_cache_invalidation() {
        let engine = Engine::default();
        let wasm1 = create_minimal_wasm();
        let mut wasm2 = wasm1.clone();
        wasm2.push(0x00); // Modify to get different hash

        // Clear cache first
        clear_cache().ok();

        // Cache first module
        let module1 = Module::new(&engine, &wasm1).expect("Should compile");
        cache_module(&wasm1, &module1).expect("Should cache");

        // Second module should not hit cache
        let result = load_cached_module(&engine, &wasm2).expect("Should not error");
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_stats() {
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        // Clear cache first
        clear_cache().ok();

        let stats_before = cache_stats().expect("Should get stats");
        assert_eq!(stats_before.num_entries, 0);

        // Add a cached module
        let module = Module::new(&engine, &wasm).expect("Should compile");
        cache_module(&wasm, &module).expect("Should cache");

        let stats_after = cache_stats().expect("Should get stats");
        assert_eq!(stats_after.num_entries, 1);
        assert!(stats_after.total_size > 0);
    }

    #[test]
    fn test_clear_cache() {
        let engine = Engine::default();
        let wasm = create_minimal_wasm();

        // Add a cached module
        let module = Module::new(&engine, &wasm).expect("Should compile");
        cache_module(&wasm, &module).expect("Should cache");

        // Verify it's there
        let stats = cache_stats().expect("Should get stats");
        assert!(stats.num_entries > 0);

        // Clear cache
        clear_cache().expect("Should clear");

        // Verify it's gone
        let stats = cache_stats().expect("Should get stats");
        assert_eq!(stats.num_entries, 0);
    }
}

use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokenizers::Tokenizer;
use tracing::debug;

const CHECKSUM_MANIFEST: &str = "checksums.sha256";
const CONFIG_FILE: &str = "config.json";
const MODEL_FILE: &str = "model.safetensors";
const TOKENIZER_FILE: &str = "tokenizer.json";

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// A local embedding model for generating vector representations of text.
pub struct EmbeddingModel {
    tokenizer: Tokenizer,
    dimensions: usize,
}

impl EmbeddingModel {
    /// Load an embedding model from a local directory.
    /// The directory must contain model weights and tokenizer files.
    pub fn load(model_path: &Path) -> Result<Self, EmbeddingError> {
        ensure_model_directory(model_path)?;
        ensure_required_files(model_path)?;
        verify_model_integrity(model_path)?;

        let tokenizer = load_tokenizer(model_path)?;
        let dimensions = load_dimensions(model_path)?;

        debug!(
            ?model_path,
            dimensions,
            "loaded fx-embeddings placeholder backend; candle BERT forward pass lands in a follow-up tied to docs/specs/phase2c-embedding-memory.md"
        );

        Ok(Self {
            tokenizer,
            dimensions,
        })
    }

    /// Generate an embedding vector for a single text input.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let token_ids = tokenize_text(&self.tokenizer, text)?;
        let values = generate_placeholder_embedding(&token_ids, text, self.dimensions);
        Ok(normalize(values))
    }

    /// Generate embedding vectors for multiple texts (batched for efficiency).
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        texts.iter().map(|text| self.embed(text)).collect()
    }

    /// Return the dimensionality of the model's output vectors.
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("embedding model not found at {0}")]
    ModelNotFound(PathBuf),
    #[error("failed to load embedding model: {0}")]
    LoadFailed(String),
    #[error("failed to tokenize text: {0}")]
    TokenizationFailed(String),
    #[error("failed to run embedding inference: {0}")]
    InferenceFailed(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

/// Verify model file checksums against a manifest.
pub fn verify_model_integrity(model_path: &Path) -> Result<(), EmbeddingError> {
    ensure_model_directory(model_path)?;
    let manifest = read_manifest(model_path)?;
    let entries = parse_manifest(&manifest)?;

    for entry in entries {
        verify_checksum_entry(model_path, &entry)?;
    }

    Ok(())
}

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let (dot, norm_a, norm_b) = a.iter().zip(b.iter()).fold(
        (0.0_f32, 0.0_f32, 0.0_f32),
        |(dot, norm_a, norm_b), (x, y)| (dot + (x * y), norm_a + (x * x), norm_b + (y * y)),
    );

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(-1.0, 1.0)
}

#[derive(Debug)]
struct ChecksumEntry {
    expected: String,
    relative_path: PathBuf,
}

fn ensure_model_directory(model_path: &Path) -> Result<(), EmbeddingError> {
    if model_path.is_dir() {
        return Ok(());
    }

    Err(EmbeddingError::ModelNotFound(model_path.to_path_buf()))
}

fn ensure_required_files(model_path: &Path) -> Result<(), EmbeddingError> {
    for file_name in [CHECKSUM_MANIFEST, CONFIG_FILE, MODEL_FILE, TOKENIZER_FILE] {
        require_file(model_path, file_name)?;
    }

    Ok(())
}

fn require_file(model_path: &Path, file_name: &str) -> Result<(), EmbeddingError> {
    let file_path = model_path.join(file_name);
    if file_path.is_file() {
        return Ok(());
    }

    Err(EmbeddingError::LoadFailed(format!(
        "required model file missing: {}",
        file_path.display()
    )))
}

fn load_tokenizer(model_path: &Path) -> Result<Tokenizer, EmbeddingError> {
    let tokenizer_path = model_path.join(TOKENIZER_FILE);
    Tokenizer::from_file(&tokenizer_path).map_err(|error| {
        EmbeddingError::LoadFailed(format!(
            "failed to load tokenizer from {}: {error}",
            tokenizer_path.display()
        ))
    })
}

fn load_dimensions(model_path: &Path) -> Result<usize, EmbeddingError> {
    let config_path = model_path.join(CONFIG_FILE);
    let config = fs::read_to_string(&config_path).map_err(EmbeddingError::IoError)?;
    let value: serde_json::Value = serde_json::from_str(&config).map_err(|error| {
        EmbeddingError::LoadFailed(format!(
            "invalid model config at {}: {error}",
            config_path.display()
        ))
    })?;

    let hidden_size = value
        .get("hidden_size")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            EmbeddingError::LoadFailed(format!(
                "config missing hidden_size at {}",
                config_path.display()
            ))
        })?;

    usize::try_from(hidden_size).map_err(|_| {
        EmbeddingError::LoadFailed(format!(
            "hidden_size does not fit in usize at {}",
            config_path.display()
        ))
    })
}

fn tokenize_text(tokenizer: &Tokenizer, text: &str) -> Result<Vec<u32>, EmbeddingError> {
    tokenizer
        .encode(text, true)
        .map(|encoding| encoding.get_ids().to_vec())
        .map_err(|error| EmbeddingError::TokenizationFailed(error.to_string()))
}

fn generate_placeholder_embedding(token_ids: &[u32], text: &str, dimensions: usize) -> Vec<f32> {
    let mut seed = Vec::with_capacity(text.len() + (token_ids.len() * 4));
    seed.extend_from_slice(text.as_bytes());

    for token_id in token_ids {
        seed.extend_from_slice(&token_id.to_le_bytes());
    }

    // TODO(spec: phase2c-embedding-memory PR1): replace this placeholder hashing backend with candle BERT mean pooling.
    expand_seed_to_vector(&seed, dimensions)
}

fn expand_seed_to_vector(seed: &[u8], dimensions: usize) -> Vec<f32> {
    let mut values = Vec::with_capacity(dimensions);
    let mut counter = 0_u64;

    while values.len() < dimensions {
        let digest = hash_with_counter(seed, counter);
        append_digest_values(&mut values, &digest, dimensions);
        counter += 1;
    }

    values
}

fn hash_with_counter(seed: &[u8], counter: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(counter.to_le_bytes());
    hasher.finalize().into()
}

fn append_digest_values(values: &mut Vec<f32>, digest: &[u8; 32], dimensions: usize) {
    for chunk in digest.chunks_exact(4) {
        if values.len() == dimensions {
            break;
        }

        let raw = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let normalized = ((raw as f32) / (u32::MAX as f32) * 2.0) - 1.0;
        values.push(normalized);
    }
}

fn normalize(values: Vec<f32>) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 {
        return values;
    }

    values.into_iter().map(|value| value / norm).collect()
}

fn read_manifest(model_path: &Path) -> Result<String, EmbeddingError> {
    let manifest_path = model_path.join(CHECKSUM_MANIFEST);
    fs::read_to_string(&manifest_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            return EmbeddingError::LoadFailed(format!(
                "checksum manifest missing: {}",
                manifest_path.display()
            ));
        }

        EmbeddingError::IoError(error)
    })
}

fn parse_manifest(manifest: &str) -> Result<Vec<ChecksumEntry>, EmbeddingError> {
    let entries = manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_manifest_line)
        .collect::<Result<Vec<_>, _>>()?;

    if entries.is_empty() {
        return Err(EmbeddingError::LoadFailed(
            "checksum manifest is empty".to_string(),
        ));
    }

    Ok(entries)
}

fn parse_manifest_line(line: &str) -> Result<ChecksumEntry, EmbeddingError> {
    let mut parts = line.split_whitespace();
    let expected = parts.next().ok_or_else(invalid_manifest_error)?;
    let relative_path = parts.next().ok_or_else(invalid_manifest_error)?;

    if parts.next().is_some() {
        return Err(invalid_manifest_error());
    }

    Ok(ChecksumEntry {
        expected: expected.to_string(),
        relative_path: PathBuf::from(relative_path),
    })
}

fn invalid_manifest_error() -> EmbeddingError {
    EmbeddingError::LoadFailed("invalid checksum manifest entry".to_string())
}

fn verify_checksum_entry(model_path: &Path, entry: &ChecksumEntry) -> Result<(), EmbeddingError> {
    let file_path = model_path.join(&entry.relative_path);
    let bytes = fs::read(&file_path).map_err(EmbeddingError::IoError)?;
    let actual = sha256_hex(&bytes);

    if actual == entry.expected {
        return Ok(());
    }

    Err(EmbeddingError::ChecksumMismatch {
        expected: entry.expected.clone(),
        actual,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        create_test_model_dir as create_model_dir,
        create_test_model_dir_with_config as create_model_dir_with_config,
    };
    use std::fs;

    #[test]
    fn cosine_similarity_is_one_for_identical_vectors() {
        let score = cosine_similarity(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_is_zero_for_orthogonal_vectors() {
        let score = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]);
        assert!(score.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_is_negative_one_for_opposite_vectors() {
        let score = cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]);
        assert!((score + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_returns_zero_for_zero_vector() {
        let score = cosine_similarity(&[0.0, 0.0], &[1.0, 2.0]);
        assert!(score.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_returns_zero_for_different_length_vectors() {
        let score = cosine_similarity(&[1.0, 2.0], &[1.0]);
        assert!(score.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_returns_zero_for_empty_vectors() {
        let score = cosine_similarity(&[], &[]);
        assert!(score.abs() < 1e-6);
    }

    #[test]
    fn embed_returns_vector_with_configured_dimensions() {
        let model_dir = create_model_dir(8);
        let model = EmbeddingModel::load(model_dir.path()).unwrap();

        let embedding = model.embed("hello world").unwrap();

        assert_eq!(embedding.len(), 8);
    }

    #[test]
    fn embed_batch_returns_one_vector_per_input() {
        let model_dir = create_model_dir(8);
        let model = EmbeddingModel::load(model_dir.path()).unwrap();

        let embeddings = model
            .embed_batch(&["hello world", "semantic search"])
            .unwrap();

        assert_eq!(embeddings.len(), 2);
    }

    #[test]
    fn embed_batch_matches_individual_embed_results() {
        let model_dir = create_model_dir(8);
        let model = EmbeddingModel::load(model_dir.path()).unwrap();

        let batch = model.embed_batch(&["hello", "world"]).unwrap();
        let hello = model.embed("hello").unwrap();
        let world = model.embed("world").unwrap();

        assert_eq!(batch, vec![hello, world]);
    }

    #[test]
    fn embed_is_deterministic_for_identical_input() {
        let model_dir = create_model_dir(8);
        let model = EmbeddingModel::load(model_dir.path()).unwrap();

        let first = model.embed("hello").unwrap();
        let second = model.embed("hello").unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn embed_output_is_unit_length() {
        let model_dir = create_model_dir(8);
        let model = EmbeddingModel::load(model_dir.path()).unwrap();

        let embedding = model.embed("hello").unwrap();
        let norm = embedding
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();

        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn verify_model_integrity_accepts_valid_checksums() {
        let model_dir = create_model_dir(8);
        let result = verify_model_integrity(model_dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn verify_model_integrity_rejects_tampered_files() {
        let model_dir = create_model_dir(8);
        fs::write(model_dir.path().join(MODEL_FILE), b"tampered").unwrap();

        let result = verify_model_integrity(model_dir.path());

        assert!(matches!(
            result,
            Err(EmbeddingError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn embedding_error_variants_have_readable_messages() {
        let errors = [
            EmbeddingError::LoadFailed("bad config".to_string()).to_string(),
            EmbeddingError::TokenizationFailed("bad token".to_string()).to_string(),
            EmbeddingError::InferenceFailed("bad inference".to_string()).to_string(),
            EmbeddingError::ChecksumMismatch {
                expected: "abc".to_string(),
                actual: "def".to_string(),
            }
            .to_string(),
        ];

        assert!(errors.iter().all(|message| !message.is_empty()));
    }

    #[test]
    fn load_returns_model_not_found_for_missing_directory() {
        let missing_path = Path::new("/definitely/missing/fx-embeddings-model");
        let result = EmbeddingModel::load(missing_path);
        assert!(matches!(result, Err(EmbeddingError::ModelNotFound(path)) if path == missing_path));
    }

    #[test]
    fn load_rejects_missing_config_file() {
        assert_missing_required_file(CONFIG_FILE);
    }

    #[test]
    fn load_rejects_missing_tokenizer_file() {
        assert_missing_required_file(TOKENIZER_FILE);
    }

    #[test]
    fn load_rejects_config_without_hidden_size() {
        let model_dir = create_model_dir_with_config(r#"{"model_type":"placeholder"}"#);
        let result = EmbeddingModel::load(model_dir.path());

        assert!(matches!(
            result,
            Err(EmbeddingError::LoadFailed(message)) if message.contains("hidden_size")
        ));
    }

    fn assert_missing_required_file(file_name: &str) {
        let model_dir = create_model_dir(8);
        fs::remove_file(model_dir.path().join(file_name)).unwrap();

        let result = EmbeddingModel::load(model_dir.path());

        assert!(matches!(
            result,
            Err(EmbeddingError::LoadFailed(message)) if message.contains(file_name)
        ));
    }
}

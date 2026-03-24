use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use fx_embeddings::{cosine_similarity, EmbeddingError, EmbeddingModel};
use thiserror::Error;

const INDEX_MAGIC: &[u8; 4] = b"FXEI";
const INDEX_VERSION: u8 = 1;
const HEADER_LEN: usize = 9;
const FOOTER_LEN: usize = 4;
const F32_LEN: usize = 4;
const U32_LEN: usize = 4;

type StoredVector = (String, Vec<f32>);
type StoredVectors = Vec<StoredVector>;
type ParsedIndex = (usize, StoredVectors);

pub type Result<T> = std::result::Result<T, EmbeddingIndexError>;

pub struct EmbeddingIndex {
    vectors: StoredVectors,
    model: Arc<EmbeddingModel>,
    /// Interior mutability: `save(&self)` clears dirty without requiring `&mut self`.
    dirty: Cell<bool>,
}

impl fmt::Debug for EmbeddingIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddingIndex")
            .field("len", &self.vectors.len())
            .field("dirty", &self.dirty.get())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum EmbeddingIndexError {
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid embedding index: {0}")]
    InvalidFormat(String),
    #[error("embedding dimension mismatch for key {key}: expected {expected}, got {actual}")]
    DimensionMismatch {
        key: String,
        expected: usize,
        actual: usize,
    },
}

impl EmbeddingIndex {
    pub fn build_from(entries: &[(String, String)], model: &Arc<EmbeddingModel>) -> Result<Self> {
        let texts = entry_texts(entries);
        let embeddings = model.embed_batch(&texts)?;
        let vectors = pair_vectors(entries, embeddings, model.dimensions())?;
        Ok(Self::new(vectors, Arc::clone(model), false))
    }

    pub fn upsert(&mut self, key: &str, text: &str) -> Result<()> {
        let embedding = self.model.embed(text)?;
        validate_dimensions(key, &embedding, self.model.dimensions())?;
        replace_vector(&mut self.vectors, key, embedding);
        self.dirty.set(true);
        Ok(())
    }

    pub fn remove(&mut self, key: &str) {
        if remove_vector(&mut self.vectors, key) {
            self.dirty.set(true);
        }
    }

    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<(String, f32)>> {
        if max_results == 0 || self.vectors.is_empty() {
            return Ok(Vec::new());
        }

        let query_embedding = self.model.embed(query)?;
        validate_dimensions("query", &query_embedding, self.model.dimensions())?;
        let mut results = scored_results(&self.vectors, &query_embedding);
        results.sort_by(compare_search_results);
        results.truncate(max_results);
        Ok(results)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        create_parent_dir(path)?;
        let dimensions = stored_dimensions(&self.vectors, self.model.dimensions())?;
        let bytes = serialize_index(&self.vectors, dimensions)?;
        fs::write(path, bytes)?;
        self.dirty.set(false);
        Ok(())
    }

    pub fn load(path: &Path, model: Arc<EmbeddingModel>) -> Result<Self> {
        let bytes = fs::read(path)?;
        let (dimensions, vectors) = deserialize_index(&bytes)?;
        validate_loaded_dimensions(dimensions, model.dimensions())?;
        validate_all_dimensions(&vectors, dimensions)?;
        Ok(Self::new(vectors, model, false))
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.get()
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    fn new(vectors: StoredVectors, model: Arc<EmbeddingModel>, dirty: bool) -> Self {
        Self {
            vectors,
            model,
            dirty: Cell::new(dirty),
        }
    }
}

fn entry_texts(entries: &[(String, String)]) -> Vec<&str> {
    entries.iter().map(|(_, text)| text.as_str()).collect()
}

fn pair_vectors(
    entries: &[(String, String)],
    embeddings: Vec<Vec<f32>>,
    expected_dimensions: usize,
) -> Result<StoredVectors> {
    if entries.len() != embeddings.len() {
        return Err(EmbeddingIndexError::InvalidFormat(
            "embedding batch size did not match entry count".to_string(),
        ));
    }

    entries
        .iter()
        .zip(embeddings)
        .map(|((key, _), embedding)| {
            validate_dimensions(key, &embedding, expected_dimensions)?;
            Ok((key.clone(), embedding))
        })
        .collect()
}

fn validate_dimensions(key: &str, vector: &[f32], expected: usize) -> Result<()> {
    if vector.len() == expected {
        return Ok(());
    }

    Err(EmbeddingIndexError::DimensionMismatch {
        key: key.to_string(),
        expected,
        actual: vector.len(),
    })
}

fn replace_vector(vectors: &mut StoredVectors, key: &str, embedding: Vec<f32>) {
    if let Some((_, vector)) = vectors
        .iter_mut()
        .find(|(existing_key, _)| existing_key == key)
    {
        *vector = embedding;
        return;
    }

    vectors.push((key.to_string(), embedding));
}

fn remove_vector(vectors: &mut StoredVectors, key: &str) -> bool {
    let before = vectors.len();
    vectors.retain(|(existing_key, _)| existing_key != key);
    before != vectors.len()
}

fn scored_results(vectors: &[StoredVector], query_embedding: &[f32]) -> Vec<(String, f32)> {
    vectors
        .iter()
        .map(|(key, embedding)| (key.clone(), cosine_similarity(query_embedding, embedding)))
        .collect()
}

fn compare_search_results(left: &(String, f32), right: &(String, f32)) -> Ordering {
    right
        .1
        .partial_cmp(&left.1)
        .unwrap_or(Ordering::Equal)
        .then_with(|| left.0.cmp(&right.0))
}

fn create_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn stored_dimensions(vectors: &[StoredVector], fallback: usize) -> Result<usize> {
    if vectors.is_empty() {
        return Ok(fallback);
    }

    let dimensions = vectors[0].1.len();
    validate_all_dimensions(vectors, dimensions)?;
    validate_loaded_dimensions(dimensions, fallback)?;
    Ok(dimensions)
}

fn validate_all_dimensions(vectors: &[StoredVector], expected: usize) -> Result<()> {
    for (key, vector) in vectors {
        validate_dimensions(key, vector, expected)?;
    }
    Ok(())
}

fn validate_loaded_dimensions(file_dimensions: usize, model_dimensions: usize) -> Result<()> {
    if file_dimensions == model_dimensions {
        return Ok(());
    }

    Err(EmbeddingIndexError::InvalidFormat(format!(
        "index dimensions {file_dimensions} do not match model dimensions {model_dimensions}"
    )))
}

fn serialize_index(vectors: &[StoredVector], dimensions: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(serialized_capacity(vectors)?);
    bytes.extend_from_slice(INDEX_MAGIC);
    bytes.push(INDEX_VERSION);
    bytes.extend_from_slice(&serialize_u32(dimensions, "index dimensions")?.to_le_bytes());

    for (key, vector) in vectors {
        write_entry(&mut bytes, key, vector)?;
    }

    bytes.extend_from_slice(&serialize_u32(vectors.len(), "entry count")?.to_le_bytes());
    Ok(bytes)
}

fn serialized_capacity(vectors: &[StoredVector]) -> Result<usize> {
    let entry_bytes = vectors.iter().try_fold(0_usize, |total, (key, vector)| {
        total
            .checked_add(serialized_entry_len(key, vector)?)
            .ok_or_else(index_size_overflow)
    })?;
    HEADER_LEN
        .checked_add(entry_bytes)
        .and_then(|total| total.checked_add(FOOTER_LEN))
        .ok_or_else(index_size_overflow)
}

fn serialized_entry_len(key: &str, vector: &[f32]) -> Result<usize> {
    let vector_bytes = vector
        .len()
        .checked_mul(F32_LEN)
        .ok_or_else(index_size_overflow)?;
    U32_LEN
        .checked_add(key.len())
        .and_then(|total| total.checked_add(vector_bytes))
        .ok_or_else(index_size_overflow)
}

fn index_size_overflow() -> EmbeddingIndexError {
    EmbeddingIndexError::InvalidFormat("serialized index size overflowed".to_string())
}

fn serialize_u32(value: usize, field: &str) -> Result<u32> {
    u32::try_from(value).map_err(|_| {
        EmbeddingIndexError::InvalidFormat(format!("{field} {value} exceeds u32::MAX"))
    })
}

fn write_entry(bytes: &mut Vec<u8>, key: &str, vector: &[f32]) -> Result<()> {
    bytes.extend_from_slice(&serialize_u32(key.len(), "key length")?.to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn deserialize_index(bytes: &[u8]) -> Result<ParsedIndex> {
    validate_minimum_length(bytes)?;
    let dimensions = read_header(bytes)?;
    let footer_start = bytes.len() - FOOTER_LEN;
    let entry_count = read_u32(bytes, footer_start)? as usize;
    let vectors = read_entries(bytes, dimensions, footer_start)?;
    validate_entry_count(vectors.len(), entry_count)?;
    Ok((dimensions, vectors))
}

fn validate_minimum_length(bytes: &[u8]) -> Result<()> {
    if bytes.len() >= HEADER_LEN + FOOTER_LEN {
        return Ok(());
    }

    Err(EmbeddingIndexError::InvalidFormat(
        "file too short to contain header and footer".to_string(),
    ))
}

fn read_header(bytes: &[u8]) -> Result<usize> {
    validate_magic(bytes)?;
    validate_version(bytes[4])?;
    Ok(read_u32(bytes, 5)? as usize)
}

fn validate_magic(bytes: &[u8]) -> Result<()> {
    if &bytes[..4] == INDEX_MAGIC {
        return Ok(());
    }

    Err(EmbeddingIndexError::InvalidFormat(
        "wrong magic bytes".to_string(),
    ))
}

fn validate_version(version: u8) -> Result<()> {
    if version == INDEX_VERSION {
        return Ok(());
    }

    Err(EmbeddingIndexError::InvalidFormat(format!(
        "unsupported version {version}"
    )))
}

fn read_entries(bytes: &[u8], dimensions: usize, footer_start: usize) -> Result<StoredVectors> {
    let mut cursor = HEADER_LEN;
    let mut vectors = Vec::new();

    while cursor < footer_start {
        let (next_cursor, entry) = read_entry(bytes, cursor, dimensions, footer_start)?;
        vectors.push(entry);
        cursor = next_cursor;
    }

    if cursor == footer_start {
        return Ok(vectors);
    }

    Err(EmbeddingIndexError::InvalidFormat(
        "entry data overlapped footer".to_string(),
    ))
}

fn read_entry(
    bytes: &[u8],
    cursor: usize,
    dimensions: usize,
    footer_start: usize,
) -> Result<(usize, StoredVector)> {
    let key_len = read_u32(bytes, cursor)? as usize;
    let key_start = cursor + U32_LEN;
    let key_end = checked_end(key_start, key_len, footer_start)?;
    let vector_end = checked_end(key_end, dimensions * F32_LEN, footer_start)?;
    let key = read_key(bytes, key_start, key_end)?;
    let vector = read_vector(bytes, key_end, dimensions)?;
    Ok((vector_end, (key, vector)))
}

fn read_key(bytes: &[u8], start: usize, end: usize) -> Result<String> {
    String::from_utf8(bytes[start..end].to_vec()).map_err(|error| {
        EmbeddingIndexError::InvalidFormat(format!("key is not valid UTF-8: {error}"))
    })
}

fn read_vector(bytes: &[u8], start: usize, dimensions: usize) -> Result<Vec<f32>> {
    let mut values = Vec::with_capacity(dimensions);
    for index in 0..dimensions {
        let offset = start + (index * F32_LEN);
        values.push(f32::from_le_bytes(read_array::<F32_LEN>(bytes, offset)?));
    }
    Ok(values)
}

fn checked_end(start: usize, len: usize, footer_start: usize) -> Result<usize> {
    let end = start
        .checked_add(len)
        .ok_or_else(|| EmbeddingIndexError::InvalidFormat("entry length overflowed".to_string()))?;

    if end <= footer_start {
        return Ok(end);
    }

    Err(EmbeddingIndexError::InvalidFormat(
        "truncated embedding entry".to_string(),
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array::<U32_LEN>(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset.checked_add(N).ok_or_else(|| {
        EmbeddingIndexError::InvalidFormat("binary offset overflowed".to_string())
    })?;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| EmbeddingIndexError::InvalidFormat("unexpected end of file".to_string()))?;

    let mut array = [0_u8; N];
    array.copy_from_slice(slice);
    Ok(array)
}

fn validate_entry_count(actual: usize, expected: usize) -> Result<()> {
    if actual == expected {
        return Ok(());
    }

    Err(EmbeddingIndexError::InvalidFormat(format!(
        "footer entry count {expected} did not match parsed entry count {actual}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    const CHECKSUM_MANIFEST: &str = "checksums.sha256";
    const CONFIG_FILE: &str = "config.json";
    const MODEL_FILE: &str = "model.safetensors";
    const TOKENIZER_FILE: &str = "tokenizer.json";
    const TEST_TOKENIZER_JSON: &str = r#"{
  "version": "1.0",
  "truncation": null,
  "padding": null,
  "added_tokens": [
    {
      "id": 0,
      "content": "[UNK]",
      "single_word": false,
      "lstrip": false,
      "rstrip": false,
      "normalized": false,
      "special": true
    }
  ],
  "normalizer": null,
  "pre_tokenizer": { "type": "Whitespace" },
  "post_processor": null,
  "decoder": null,
  "model": {
    "type": "WordLevel",
    "vocab": {
      "[UNK]": 0,
      "hello": 1,
      "world": 2,
      "memory": 3,
      "search": 4,
      "semantic": 5,
      "auth": 6,
      "decision": 7,
      "oauth": 8,
      "notes": 9
    },
    "unk_token": "[UNK]"
  }
}"#;

    #[test]
    fn build_from_creates_index_with_correct_entry_count() {
        let model = test_model(8);
        let entries = vec![
            entry("alpha", "hello world"),
            entry("beta", "semantic search"),
        ];

        let index = EmbeddingIndex::build_from(&entries, &model).expect("build index");

        assert_eq!(index.len(), 2);
        assert!(!index.is_dirty());
    }

    #[test]
    fn upsert_adds_new_entry_and_updates_existing_entry() {
        let model = test_model(8);
        let mut index = empty_index(&model);

        index.upsert("alpha", "hello world").expect("insert entry");
        let first_vector = vector_for(&index, "alpha");

        index
            .upsert("alpha", "semantic search")
            .expect("update entry");
        let second_vector = vector_for(&index, "alpha");

        assert_eq!(index.len(), 1);
        assert_ne!(first_vector, second_vector);
        assert!(index.is_dirty());
    }

    #[test]
    fn remove_deletes_entry_and_search_no_longer_finds_it() {
        let model = test_model(8);
        let entries = vec![
            entry("keep", "hello world"),
            entry("drop", "auth decision notes"),
        ];
        let mut index = EmbeddingIndex::build_from(&entries, &model).expect("build index");

        index.remove("drop");
        let results = index.search("auth decision notes", 5).expect("search");

        assert!(results.iter().all(|(key, _)| key != "drop"));
        assert_eq!(index.len(), 1);
        assert!(index.is_dirty());
    }

    #[test]
    fn remove_nonexistent_key_does_not_set_dirty() {
        let model = test_model(8);
        let entries = vec![entry("alpha", "hello world")];
        let mut index = EmbeddingIndex::build_from(&entries, &model).expect("build index");

        index.remove("nonexistent");

        assert!(!index.is_dirty());
    }

    #[test]
    fn search_returns_results_sorted_by_similarity_descending() {
        let model = test_model(8);
        let entries = vec![
            entry("alpha", "hello world"),
            entry("beta", "semantic search"),
            entry("gamma", "auth decision notes"),
        ];
        let index = EmbeddingIndex::build_from(&entries, &model).expect("build index");
        let expected = expected_ranking(&entries, &model, "hello world");

        let results = index.search("hello world", 3).expect("search");

        assert_eq!(results, expected);
    }

    #[test]
    fn search_respects_max_results_limit() {
        let model = test_model(8);
        let entries = vec![
            entry("alpha", "hello world"),
            entry("beta", "semantic search"),
            entry("gamma", "auth decision notes"),
        ];
        let index = EmbeddingIndex::build_from(&entries, &model).expect("build index");

        let results = index.search("hello world", 2).expect("search");

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_returns_empty_vec_for_empty_index() {
        let model = test_model(8);
        let index = empty_index(&model);

        let results = index.search("hello world", 5).expect("search");

        assert!(results.is_empty());
    }

    #[test]
    fn search_with_zero_max_results_returns_empty_vec() {
        let model = test_model(8);
        let entries = vec![entry("alpha", "hello world")];
        let index = EmbeddingIndex::build_from(&entries, &model).expect("build index");

        let results = index.search("hello world", 0).expect("search");

        assert!(results.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip_preserves_all_entries_and_vectors() {
        let temp_dir = TempDir::new().expect("tempdir");
        let path = temp_dir.path().join("memory").join("embeddings.bin");
        let model = test_model(8);
        let entries = vec![
            entry("alpha", "hello world"),
            entry("beta", "semantic search"),
        ];
        let index = EmbeddingIndex::build_from(&entries, &model).expect("build index");

        index.save(&path).expect("save index");
        let loaded = EmbeddingIndex::load(&path, Arc::clone(&model)).expect("load index");

        assert_eq!(sorted_vectors(&index), sorted_vectors(&loaded));
        assert!(!loaded.is_dirty());
    }

    #[test]
    fn save_and_load_reject_corrupted_file_with_wrong_magic_bytes() {
        let model = test_model(8);
        let error = load_error_for_bytes(b"BAD!\x01\x08\x00\x00\x00\x00\x00\x00\x00", &model);

        assert_invalid_format_message(error, "wrong magic bytes");
    }

    #[test]
    fn save_and_load_reject_corrupted_file_with_unsupported_version() {
        let model = test_model(8);
        let bytes = header_only_index_bytes(8, 2, 0);

        let error = load_error_for_bytes(&bytes, &model);

        assert_invalid_format_message(error, "unsupported version 2");
    }

    #[test]
    fn save_and_load_reject_corrupted_file_with_truncated_entry_data() {
        let model = test_model(8);
        let entries = vec![entry("alpha", "hello world")];
        let mut bytes = serialized_index_bytes(&entries, &model);
        let footer = bytes.split_off(bytes.len() - FOOTER_LEN);
        bytes.pop().expect("remove truncated byte");
        bytes.extend_from_slice(&footer);

        let error = load_error_for_bytes(&bytes, &model);

        assert_invalid_format_message(error, "truncated embedding entry");
    }

    #[test]
    fn save_and_load_reject_corrupted_file_with_footer_count_mismatch() {
        let model = test_model(8);
        let entries = vec![entry("alpha", "hello world")];
        let mut bytes = serialized_index_bytes(&entries, &model);
        let footer_start = bytes.len() - FOOTER_LEN;
        bytes[footer_start..].copy_from_slice(&2_u32.to_le_bytes());

        let error = load_error_for_bytes(&bytes, &model);

        assert_invalid_format_message(
            error,
            "footer entry count 2 did not match parsed entry count 1",
        );
    }

    #[test]
    fn save_and_load_reject_corrupted_file_with_invalid_utf8_key() {
        let model = test_model(8);
        let entries = vec![entry("alpha", "hello world")];
        let mut bytes = serialized_index_bytes(&entries, &model);
        let key_start = HEADER_LEN + U32_LEN;
        bytes[key_start] = 0xFF;

        let error = load_error_for_bytes(&bytes, &model);

        assert_invalid_format_message(error, "key is not valid UTF-8");
    }

    #[test]
    fn save_and_load_reject_corrupted_file_with_dimension_mismatch() {
        let model = test_model(8);
        let bytes = header_only_index_bytes(768, INDEX_VERSION, 0);

        let error = load_error_for_bytes(&bytes, &model);

        assert_invalid_format_message(
            error,
            "index dimensions 768 do not match model dimensions 8",
        );
    }

    #[test]
    fn is_dirty_changes_after_upsert_and_save() {
        let temp_dir = TempDir::new().expect("tempdir");
        let path = temp_dir.path().join("embeddings.bin");
        let model = test_model(8);
        let mut index = empty_index(&model);

        assert!(!index.is_dirty());
        index.upsert("alpha", "hello world").expect("upsert");
        assert!(index.is_dirty());
        index.save(&path).expect("save");
        assert!(!index.is_dirty());
    }

    #[test]
    fn len_and_is_empty_report_current_state() {
        let model = test_model(8);
        let mut index = empty_index(&model);

        assert_eq!(index.len(), 0);
        assert!(index.is_empty());

        index.upsert("alpha", "hello world").expect("upsert");

        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());
    }

    fn empty_index(model: &Arc<EmbeddingModel>) -> EmbeddingIndex {
        EmbeddingIndex::build_from(&[], model).expect("build empty index")
    }

    fn serialized_index_bytes(
        entries: &[(String, String)],
        model: &Arc<EmbeddingModel>,
    ) -> Vec<u8> {
        let index = EmbeddingIndex::build_from(entries, model).expect("build index");
        serialize_index(&index.vectors, model.dimensions()).expect("serialize index")
    }

    fn header_only_index_bytes(dimensions: u32, version: u8, entry_count: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + FOOTER_LEN);
        bytes.extend_from_slice(INDEX_MAGIC);
        bytes.push(version);
        bytes.extend_from_slice(&dimensions.to_le_bytes());
        bytes.extend_from_slice(&entry_count.to_le_bytes());
        bytes
    }

    fn load_error_for_bytes(bytes: &[u8], model: &Arc<EmbeddingModel>) -> EmbeddingIndexError {
        let temp_dir = TempDir::new().expect("tempdir");
        let path = temp_dir.path().join("embeddings.bin");
        fs::write(&path, bytes).expect("write file");
        EmbeddingIndex::load(&path, Arc::clone(model)).expect_err("expected load failure")
    }

    fn assert_invalid_format_message(error: EmbeddingIndexError, expected: &str) {
        assert!(
            matches!(error, EmbeddingIndexError::InvalidFormat(message) if message.contains(expected))
        );
    }

    fn entry(key: &str, value: &str) -> (String, String) {
        (key.to_string(), value.to_string())
    }

    fn vector_for(index: &EmbeddingIndex, key: &str) -> Vec<f32> {
        index
            .vectors
            .iter()
            .find(|(existing_key, _)| existing_key == key)
            .map(|(_, vector)| vector.clone())
            .expect("vector for key")
    }

    fn expected_ranking(
        entries: &[(String, String)],
        model: &Arc<EmbeddingModel>,
        query: &str,
    ) -> Vec<(String, f32)> {
        let query_embedding = model.embed(query).expect("query embedding");
        let mut scores: Vec<_> = entries
            .iter()
            .map(|(key, value)| {
                let embedding = model.embed(value).expect("entry embedding");
                (key.clone(), cosine_similarity(&query_embedding, &embedding))
            })
            .collect();
        scores.sort_by(compare_search_results);
        scores
    }

    fn sorted_vectors(index: &EmbeddingIndex) -> StoredVectors {
        let mut vectors = index.vectors.clone();
        vectors.sort_by(|left, right| left.0.cmp(&right.0));
        vectors
    }

    fn test_model(dimensions: usize) -> Arc<EmbeddingModel> {
        let model_dir = create_model_dir(dimensions);
        Arc::new(EmbeddingModel::load(model_dir.path()).expect("load test model"))
    }

    fn create_model_dir(dimensions: usize) -> TempDir {
        let temp_dir = TempDir::new().expect("tempdir");
        write_model_file(
            &temp_dir,
            CONFIG_FILE,
            &format!("{{\"hidden_size\": {dimensions}}}"),
        );
        write_model_file(&temp_dir, TOKENIZER_FILE, TEST_TOKENIZER_JSON);
        write_model_file(&temp_dir, MODEL_FILE, "placeholder weights");
        write_manifest(temp_dir.path());
        temp_dir
    }

    fn write_model_file(temp_dir: &TempDir, file_name: &str, contents: &str) {
        fs::write(temp_dir.path().join(file_name), contents).expect("write model file");
    }

    fn write_manifest(model_path: &Path) {
        let mut lines = Vec::new();
        for file_name in [CONFIG_FILE, TOKENIZER_FILE, MODEL_FILE] {
            let bytes = fs::read(model_path.join(file_name)).expect("read model file");
            let checksum = sha256_hex(&bytes);
            lines.push(format!("{checksum}  {file_name}"));
        }

        fs::write(model_path.join(CHECKSUM_MANIFEST), lines.join("\n")).expect("write manifest");
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::Digest;

        let digest = sha2::Sha256::digest(bytes);
        format!("{digest:x}")
    }
}

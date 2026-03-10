# Phase 2c: Embedding-Based Memory Search

## Summary

Replace `search_relevant()`'s substring matching with vector similarity search using a local embedding model. Currently memory search is keyword-based — "what was the auth decision?" won't find a memory stored as "switched to PKCE OAuth flow for ChatGPT credentials." Embeddings solve this by matching semantic meaning, not literal text.

## Current State

- `MemoryProvider` trait in `fx-core/src/memory.rs` defines `search()` (substring) and `search_relevant()` (multi-term keyword ranking)
- `JsonFileMemory` in `fx-memory` implements both — all substring-based
- `memory_read` tool does exact key lookup, `memory_list` returns all, no semantic search tool exists
- Memory entries: key-value pairs with metadata (access count, timestamps, tags, decay weight)
- Max 1,000 entries, 10KB per value
- File-backed: `~/.fawx/memory/memory.json`

## Design

### Architecture

```
┌──────────────────┐     ┌──────────────────┐
│   memory_search  │────▶│  EmbeddingIndex   │
│   (new tool)     │     │  (fx-memory)      │
└──────────────────┘     └────────┬──────────┘
                                  │
                         ┌────────▼──────────┐
                         │  EmbeddingModel    │
                         │  (fx-embeddings)   │
                         │  - candle + nomic  │
                         │  - local inference │
                         └───────────────────┘
```

**Engine-level, not WASM.** Embedding inference needs direct hardware access (CPU SIMD, optional GPU). WASM would add overhead and complexity for no benefit.

### New Crate: `fx-embeddings`

Handles model loading and text → vector conversion:

```rust
pub struct EmbeddingModel {
    model: candle_nn::Module,
    tokenizer: tokenizers::Tokenizer,
    dimensions: usize,  // e.g., 768 for nomic-embed-text-v1.5
}

impl EmbeddingModel {
    /// Load model from local path or download on first use.
    pub fn load(model_path: &Path) -> Result<Self>;

    /// Generate embedding vector for text.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Batch embed multiple texts (more efficient).
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}
```

**Model choice: nomic-embed-text-v1.5**
- 768 dimensions, 137M params (quantized ~70MB)
- Apache 2.0 license
- Runs on CPU via candle (no Python, no ONNX runtime needed)
- Good quality for its size — outperforms many larger models on MTEB
- Alternative: all-MiniLM-L6-v2 (384 dims, 23MB, faster but lower quality)

### Embedding Index in `fx-memory`

```rust
pub struct EmbeddingIndex {
    vectors: Vec<(String, Vec<f32>)>,  // (memory_key, embedding)
    model: Arc<EmbeddingModel>,
    dirty: bool,  // needs save
}

impl EmbeddingIndex {
    /// Build index from existing memory entries.
    pub fn build_from(memory: &dyn MemoryProvider, model: &EmbeddingModel) -> Result<Self>;

    /// Add/update embedding for a key.
    pub fn upsert(&mut self, key: &str, text: &str) -> Result<()>;

    /// Remove embedding for a key.
    pub fn remove(&mut self, key: &str);

    /// Search by semantic similarity. Returns top-k results with scores.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<(String, f32)>>;

    /// Save index to disk.
    pub fn save(&self, path: &Path) -> Result<()>;

    /// Load index from disk.
    pub fn load(path: &Path, model: Arc<EmbeddingModel>) -> Result<Self>;
}
```

**Storage:** `~/.fawx/memory/embeddings.bin` — simple binary format (key + f32 vector pairs). No external vector DB needed at this scale (1,000 entries × 768 dims = ~3MB).

**Similarity:** Cosine similarity, computed inline. At 1,000 entries, brute-force search takes <1ms. No need for HNSW or other ANN structures.

### New Tool: `memory_search`

```json
{
  "name": "memory_search",
  "description": "Search agent memory by meaning. Finds semantically related memories even without exact keyword matches.",
  "parameters": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "Natural language search query" },
      "max_results": { "type": "integer", "description": "Maximum results to return (default: 5)" }
    },
    "required": ["query"]
  }
}
```

Returns:
```
Found 3 relevant memories:

1. [auth_decision] (score: 0.89)
   Switched to PKCE OAuth flow for ChatGPT credentials. API key approach deprecated.

2. [security_review] (score: 0.76)
   Bearer token stored in encrypted credential store, not config.toml.

3. [api_keys] (score: 0.71)
   Claude uses setup-token flow. ChatGPT uses PKCE. Both stored in fx-auth.
```

### Integration with Existing Memory Operations

**On `memory_write`:** After writing to JSON store, also call `index.upsert(key, value)` to update the embedding.

**On `memory_delete`:** After deleting from JSON store, call `index.remove(key)`.

**On startup:** Load existing index from disk. If index is stale (memory.json is newer), rebuild incrementally (embed only new/changed keys).

**`search_relevant` upgrade:** Replace the keyword-based implementation with embedding search when the model is loaded. Fall back to keyword if embeddings unavailable (model not downloaded, load failure).

### Model Lifecycle

**First run:**
1. Check `~/.fawx/models/nomic-embed-text-v1.5/` exists
2. If not, download from HuggingFace (~70MB quantized)
3. Load into memory (~200MB RAM with quantization)

**Config:**
```toml
[memory]
# Enable embedding-based search (default: true if model available)
embeddings = true

# Model path (default: ~/.fawx/models/nomic-embed-text-v1.5)
# embedding_model = "~/.fawx/models/nomic-embed-text-v1.5"

# Embedding dimensions (auto-detected from model, override for custom models)
# embedding_dimensions = 768
```

**Graceful degradation:** If the model can't load (out of memory, missing files), fall back to keyword search silently. Log a warning. Never block startup.

## Security

### Model Integrity
- Verify model file checksums on load (SHA-256)
- Store expected hashes in a manifest file alongside the model
- Reject models that don't match (prevents tampered model injection)
- Download only from known URLs (HuggingFace CDN)

### Resource Budgeting
- Model loading: ~200MB RAM, ~2s on modern CPU
- Per-query inference: ~5ms for query embedding, ~1ms for similarity search
- Index rebuild: ~30s for 1,000 entries (batch embedding)
- Cap batch size to prevent OOM on large memory stores

### Poisoning Defense
- Embeddings are derived from memory content the agent wrote — not external input
- No user-supplied vectors (all generated from text via the local model)
- Model is read-only after download — agent cannot modify it
- Tier 3 protection on model directory (immutable)

## Implementation Plan

### PR 1: fx-embeddings crate (~300 lines)
- `EmbeddingModel` struct with candle integration
- `embed()` and `embed_batch()` methods
- Model loading with checksum verification
- Download helper (HuggingFace CDN)
- Tests: embed produces correct dimensions, batch matches single, checksum verification

### PR 2: EmbeddingIndex in fx-memory (~250 lines)
- `EmbeddingIndex` struct with cosine similarity
- `build_from()`, `upsert()`, `remove()`, `search()`
- Binary serialization (save/load)
- Tests: upsert + search finds related, remove excludes, save/load roundtrip

### PR 3: Integration + memory_search tool (~200 lines)
- Wire `EmbeddingIndex` into `JsonFileMemory`
- Hook `memory_write`/`memory_delete` to update index
- Add `memory_search` tool to `fawx_tool_definitions()`
- `search_relevant()` upgrade (embedding with keyword fallback)
- Config integration
- Startup: model loading, index loading/rebuilding
- Tests: end-to-end semantic search, fallback behavior

## Dependencies

- `candle-core` — tensor operations (from HuggingFace, Rust-native)
- `candle-nn` — neural network layers
- `candle-transformers` — transformer model implementations
- `tokenizers` — HuggingFace tokenizer (Rust, fast)
- `hf-hub` — model download from HuggingFace

**Justification:** These are the Rust-native ML inference stack from HuggingFace. No Python, no ONNX, no C++ dependencies. candle is well-maintained (60k+ GitHub stars across the HF ecosystem), Apache 2.0 licensed, and purpose-built for this use case.

## Size Estimate

~750 lines across 3 PRs + ~300 lines of tests. New crate + modifications to fx-memory and fx-tools.

## Risks

- **Model size:** 70MB download + 200MB RAM. Fine for desktop, tight for VPS with 8GB. Quantization helps.
- **candle maturity:** Newer than PyTorch/ONNX, but actively developed by HuggingFace. Risk mitigated by graceful fallback.
- **First-run download:** 70MB download on first use. Need good UX (progress bar, skip option).
- **Cold start:** ~2s model load. Acceptable for startup, not for per-request loading. Load once, keep in memory.

## Future

- GPU acceleration via candle (CUDA/Metal) — zero code changes, just feature flags
- Custom fine-tuned embedding model trained on Fawx-specific data
- Cross-session memory federation (fleet knowledge sharing, Wave 9)
- Hierarchical memory: embeddings for long-term, keywords for short-term

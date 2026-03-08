# Spec: fawx-skill-memory (Semantic Search)

**Status:** Draft  
**Date:** 2026-03-08  
**Repo:** `fawxai/fawx-skill-memory`

---

## 1. Problem

fx-journal provides keyword-based journal search. For effective memory recall, Fawx needs semantic search — find relevant memories by meaning, not exact keywords.

## 2. Approach

WASM skill that:
1. Indexes memory/journal files using embeddings
2. Accepts natural language queries
3. Returns ranked results with snippets and source citations

## 3. Tools

```json
{
  "name": "memory_search",
  "description": "Semantically search memory files for relevant context",
  "parameters": {
    "query": "string — natural language search query",
    "max_results": "integer — max results to return (default: 5)",
    "min_score": "number — minimum similarity threshold (default: 0.3)"
  }
}

{
  "name": "memory_index",
  "description": "Re-index memory files for search",
  "parameters": {
    "paths": "array of strings — file paths to index (default: all memory files)"
  }
}
```

## 4. Embedding Strategy

**Option A: API-based** (simpler, requires network)
- Call OpenAI `text-embedding-3-small` or Anthropic embeddings API
- ~$0.02 per 1M tokens — negligible cost
- Store embeddings in local file (`~/.fawx/memory/embeddings.json`)

**Option B: Local model** (offline, more complex)
- Bundle a small ONNX embedding model (e.g., `all-MiniLM-L6-v2`)
- Run inference in WASM — may be slow but zero API cost
- ~23MB model file

**Recommendation:** Option A for v1 (ship fast), Option B as future upgrade.

## 5. Index Format

```json
{
  "version": 1,
  "model": "text-embedding-3-small",
  "chunks": [
    {
      "path": "memory/2026-03-08.md",
      "line_start": 1,
      "line_end": 15,
      "text": "chunk text...",
      "embedding": [0.023, -0.041, ...]
    }
  ]
}
```

Chunking: split files by markdown headers or fixed ~500 token windows with overlap.

## 6. Testing

- Index a test file, search returns relevant chunks
- Score ordering is correct
- Empty index returns no results
- Chunk boundary handling
- Re-index updates changed files

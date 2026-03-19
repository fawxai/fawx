# Spec: fawx-skill-vision (Image Analysis)

**Status:** Draft  
**Date:** 2026-03-08  
**Repo:** `fawxai/fawx-skill-vision`  
**Depends on:** Telegram image receive

---

## 1. Problem

Fawx can't analyze images. Users send screenshots, photos, diagrams — Fawx needs to understand them.

## 2. Approach

WASM skill that sends images to a vision-capable LLM (Claude, GPT-4o) and returns the analysis.

## 3. Tool

```json
{
  "name": "analyze_image",
  "description": "Analyze an image using a vision model. Returns a description and any extracted information.",
  "parameters": {
    "image_path": "string — local file path to the image",
    "prompt": "string — what to look for or analyze (optional)",
    "model": "string — vision model to use (default: claude-sonnet-4-6)"
  }
}
```

## 4. Implementation

1. Read image file from disk (host_api file read)
2. Base64 encode
3. Construct multimodal API request (Anthropic or OpenAI format)
4. Send via host_api HTTP (or use parent's router if host_api supports LLM calls)
5. Return text description

### API Format (Anthropic)

```json
{
  "model": "claude-sonnet-4-6-20250929",
  "messages": [{
    "role": "user",
    "content": [
      {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "..."}},
      {"type": "text", "text": "Describe this image"}
    ]
  }]
}
```

## 5. Supported Formats

- JPEG, PNG, GIF, WebP
- Max size: 20MB (Anthropic limit)
- Auto-resize if larger

## 6. Testing

- Mock API server, verify request format
- Handle missing file gracefully
- Handle unsupported format
- Handle API error (rate limit, invalid image)
- Prompt inclusion in request

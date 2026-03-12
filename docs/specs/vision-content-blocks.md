# Vision Content Blocks — Native Image Support

## Problem

When Telegram receives a photo, it downloads the file and prepends `[Image: /path/to/file.jpg]` as text. The model sees this as a string and tries to use the vision WASM skill, which fails because it expects URLs or base64 data URIs — not local file paths.

The correct fix: pass images as native multimodal content blocks directly to the LLM, bypassing the vision skill entirely. Both Claude and GPT-4o support image content blocks natively.

## Changes

### 1. fx-llm/src/types.rs — Add `Image` variant to `ContentBlock`

```rust
/// Structured content within a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: Value },
    /// Base64-encoded image content block for vision-capable models.
    Image {
        /// MIME type (e.g., "image/jpeg", "image/png").
        media_type: String,
        /// Base64-encoded image data (no data URI prefix).
        data: String,
    },
}
```

Add a constructor to `Message`:

```rust
impl Message {
    /// Create a user message with text and images.
    pub fn user_with_images(text: impl Into<String>, images: Vec<(String, String)>) -> Self {
        let mut content: Vec<ContentBlock> = images
            .into_iter()
            .map(|(media_type, data)| ContentBlock::Image { media_type, data })
            .collect();
        let text = text.into();
        if !text.is_empty() {
            content.push(ContentBlock::Text { text });
        }
        Self {
            role: MessageRole::User,
            content,
        }
    }
}
```

### 2. fx-llm/src/anthropic.rs — Map Image blocks

Add `Image` variant to `AnthropicContentBlock`:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    // ... existing variants ...
    Image {
        source: AnthropicImageSource,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: String,  // always "base64"
    media_type: String,
    data: String,
}
```

Update `map_content_to_anthropic`:

```rust
ContentBlock::Image { media_type, data } => Ok(AnthropicContentBlock::Image {
    source: AnthropicImageSource {
        source_type: "base64".to_string(),
        media_type: media_type.clone(),
        data: data.clone(),
    },
}),
```

### 3. fx-llm/src/openai.rs — Map Image blocks

OpenAI uses `image_url` with data URI format:

```json
{
    "type": "image_url",
    "image_url": {
        "url": "data:image/jpeg;base64,{data}"
    }
}
```

Map `ContentBlock::Image` to this format in the OpenAI message builder.

### 4. fx-cli/src/http_serve.rs — Encode photos as base64

Replace `build_text_with_photos` to return structured data instead of text-prefixed strings:

```rust
/// Encoded image ready for content blocks.
struct EncodedImage {
    media_type: String,
    base64_data: String,
}

/// Download and base64-encode photos from incoming message.
async fn encode_photos(
    telegram: &TelegramChannel,
    incoming: &mut IncomingMessage,
) -> Vec<EncodedImage> {
    if incoming.photos.is_empty() {
        return Vec::new();
    }
    let media_dir = media_inbound_dir();
    if let Err(e) = std::fs::create_dir_all(&media_dir) {
        tracing::error!("Failed to create media dir: {e}");
        return Vec::new();
    }
    let mut images = Vec::new();
    for photo in &mut incoming.photos {
        match telegram.download_file(&photo.file_id, incoming.message_id, &media_dir).await {
            Ok(path) => {
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        use base64::Engine;
                        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        images.push(EncodedImage {
                            media_type: photo.mime_type.clone(),
                            base64_data: data,
                        });
                        photo.file_path = Some(path);
                    }
                    Err(e) => tracing::error!("Failed to read photo file: {e}"),
                }
            }
            Err(e) => tracing::error!("Failed to download photo {}: {e}", photo.file_id),
        }
    }
    images
}
```

### 5. fx-cli/src/headless.rs — Accept images in UserInput

Add an `images` field to `UserInput` (or a new parameter to `process_message_for_source`):

Option A (simpler): Add images to `UserInput`:
```rust
pub struct UserInput {
    pub text: String,
    pub source: InputSource,
    pub timestamp: u64,
    pub context_id: Option<String>,
    /// Base64-encoded images: Vec<(media_type, base64_data)>
    pub images: Vec<(String, String)>,
}
```

Option B (cleaner): New method `process_message_with_images` that builds `Message::user_with_images`.

### 6. Wire the pipeline

In `handle_telegram_poll_update` (http_serve.rs):
- Call `encode_photos()` instead of `build_text_with_photos()`
- Pass images through to `process_and_route_message` (needs new parameter)
- In `build_perception_snapshot`, if images present, use `Message::user_with_images`

## Testing

1. Unit test: `ContentBlock::Image` serializes correctly for Anthropic format
2. Unit test: `ContentBlock::Image` serializes correctly for OpenAI format
3. Unit test: `Message::user_with_images` creates correct content blocks
4. Unit test: `encode_photos` handles empty photos, download failure, read failure
5. Integration test (ignored): Telegram photo → base64 → model response

## Not in scope
- URL-based image references (only base64 for now)
- Image generation / outbound images
- Non-Telegram channels

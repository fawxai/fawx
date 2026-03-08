# Spec: Telegram Image Receive

**Status:** Draft  
**Date:** 2026-03-08  

---

## 1. Problem

fx-channel-telegram only handles text messages. When a user sends an image (photo), it's silently dropped. Fawx needs to receive images to enable vision analysis.

## 2. Goals

1. **Receive photos** from Telegram messages
2. **Download to local storage** with unique filenames
3. **Pass file path** to the agentic loop alongside any caption text
4. **Handle multiple photos** in a single message (Telegram sends as array of sizes)

## 3. Architecture

### Changes to fx-channel-telegram

#### Update `parse_update()` to handle photos

Telegram sends photos as an array of `PhotoSize` objects (different resolutions). Pick the largest.

```rust
pub struct IncomingMessage {
    pub chat_id: i64,
    pub from: Option<String>,
    pub text: String,
    pub photos: Vec<PhotoAttachment>,  // NEW
}

pub struct PhotoAttachment {
    pub file_id: String,
    pub file_path: PathBuf,  // Local path after download
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
}
```

#### Download flow

1. Extract `photo` array from Telegram update
2. Pick largest by `width * height`
3. Call `getFile` API → get `file_path`
4. Download from `https://api.telegram.org/file/bot<token>/<file_path>`
5. Save to `~/.fawx/media/inbound/<message_id>-<file_id>.jpg`
6. Attach local path to `IncomingMessage`

#### New methods on TelegramChannel

```rust
/// Download a file by file_id from Telegram servers.
pub async fn download_file(
    &self,
    file_id: &str,
    dest_dir: &Path,
) -> Result<PathBuf, TelegramError>;
```

### Changes to HeadlessApp / polling loop

In `handle_telegram_update()`:
- If `incoming.photos` is non-empty, prepend file paths to the message text
- Format: `[Image: /path/to/file.jpg]\n<caption text>`
- The agentic loop receives this as a regular message — vision skill handles analysis

### Media directory

```
~/.fawx/media/
├── inbound/    # Photos received from channels
└── outbound/   # Generated images (future)
```

Create on first use. Cleanup: delete files older than 7 days (configurable).

## 4. Testing

- Parse update with photo array, extract largest
- Download file mock (mock HTTP server returns test image)
- Handle missing photo (graceful skip)
- Handle caption + photo combination
- Media directory creation
- File naming uniqueness

## 5. File Touchpoints

- **Modify:** `engine/crates/fx-channel-telegram/src/lib.rs` (IncomingMessage, download_file, parse photos)
- **Modify:** `engine/crates/fx-cli/src/http_serve.rs` (handle_telegram_update passes photos)
- **Modify:** `engine/crates/fx-config/` (media_dir config)

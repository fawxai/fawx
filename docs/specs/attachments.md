# Attachments — Images, Files, and Media in Chat

**Status:** Draft
**Issue:** N/A
**Priority:** Ship blocker (pre-OSS launch)

## Problem

Users cannot attach images, files, or documents through the Fawx GUI (macOS or iOS). The backend already supports image attachments (base64 `ImagePayload` on the message API), and the LLM providers handle vision natively. But the GUIs have no UI to select, preview, or send attachments. Beyond images, users need to attach text files and PDFs for context (e.g., a marketing brief, a CSV of contacts, a style guide).

## Current State

### What works today:
- **Backend API:** `POST /v1/sessions/:id/messages` accepts `images: [ImagePayload]` (base64 + media_type)
- **LLM layer:** `ContentBlock::Image` serializes correctly for Anthropic, OpenAI, and OpenAI Responses API
- **Telegram channel:** Image receive works (photo parsing + download, PR #1249)
- **Swift models:** `ImagePayload` struct exists, `sendMessageStream()` accepts `images` parameter
- **TUI:** No attachment support (terminal limitation, acceptable for now)

### What's missing:
- **Swift GUI:** No picker UI, no preview, no way to attach anything to a message
- **File/document attachments:** Only images are supported at the API level. No PDFs, text files, CSVs, etc.
- **Video:** No support at any layer
- **Drag and drop:** Not implemented
- **Paste from clipboard:** Not implemented (images from ⌘V)

## Phase 1 Scope (ship blocker)

### 1A: Image Attachments

Get images working end-to-end in both macOS and iOS apps.

#### Swift UI Changes:
1. **Attachment button** in the composer bar (📎 or + icon, left of text field)
   - macOS: triggers `NSOpenPanel` with image + document UTTypes
   - iOS: triggers `PHPickerViewController` (photo library), camera, or file picker
2. **Paste support** — ⌘V in the composer pastes clipboard images
3. **Drag and drop** — drop images/files onto the chat view or composer (macOS)
4. **Preview strip** — horizontal row of thumbnails/chips above the composer showing queued attachments
   - Each item has an ✕ remove button
   - Images show thumbnail; files show icon + filename
   - Tapping/clicking an image shows full-size preview
5. **Send with attachments** — when user sends, encode images as base64 `ImagePayload` and pass to `sendMessageStream()`
6. **Display in chat** — render `ContentBlock::Image` blocks in the message bubble as inline images

#### Image Constraints:
- Max file size: 5MB per image (after resize). Resize larger images to fit.
- Max attachments per message: 10 (images + files combined)
- Supported formats: JPEG, PNG, GIF, WebP (matches `SUPPORTED_IMAGE_MEDIA_TYPES` in backend)
- Images are base64-encoded in the JSON body. No multipart upload for Phase 1.
- Client-side resize: if image > 5MB, resize to fit within 2048x2048 maintaining aspect ratio, JPEG compression at quality 0.8

### 1B: Text File Attachments

Support plain text, code, markup, and data files in chat.

#### Supported Types:
- Plain text: `.txt`, `.md`, `.csv`, `.json`, `.xml`, `.yaml`, `.yml`
- Code: `.py`, `.rs`, `.swift`, `.kt`, `.js`, `.ts`, `.html`, `.css`, `.sh`, `.toml`
- Markup: `.html`, `.htm`
- Data: `.csv`, `.tsv`, `.log`

#### How it works:
- **No new backend content block needed.** Read file contents on the Swift client, inject as text with a filename header.
- The message body sent to the API becomes:
  ```
  [file: report.csv]
  name,email,status
  Alice,alice@example.com,active
  Bob,bob@example.com,churned
  [/file: report.csv]

  Analyze this customer list and identify churn patterns.
  ```
- This approach uses the existing text message path; the LLM sees the file contents naturally.
- Files are read entirely on the client. No server-side storage for Phase 1.

#### Text File Constraints:
- Max file size: 500KB per text file (text content is large in token count)
- UTF-8 only. Binary files rejected with user feedback.
- File content is injected into the message text, not a separate content block.

### 1C: PDF Attachments

Support PDF documents in chat.

#### Backend Changes (Rust):
1. **New `ContentBlock::Document` variant** in `fx-llm/src/types.rs`:
   ```rust
   Document {
       /// MIME type (e.g., "application/pdf")
       media_type: String,
       /// Base64-encoded document data
       data: String,
       /// Optional filename for display
       filename: Option<String>,
   }
   ```
2. **Anthropic serialization** — Anthropic accepts PDFs natively as `document` content blocks with `source.type = "base64"`. Map `ContentBlock::Document` directly.
3. **OpenAI fallback** — OpenAI doesn't accept PDFs natively. Extract text using the `pdf-extract` crate and inject as `ContentBlock::Text`. Fall back gracefully.
4. **OpenAI Responses API** — same text extraction fallback.
5. **API change** — extend `SendMessageBody` to accept `documents: [DocumentPayload]` alongside `images`.

#### Swift Changes:
1. **DocumentPayload** model — mirrors ImagePayload: `data` (base64), `mediaType`, `filename`
2. **PDF preview in chat** — render first page as thumbnail, tap/click to open in system viewer
3. **Picker** — the same 📎 button opens file picker filtered to PDF UTType (alongside images)

#### PDF Constraints:
- Max file size: 10MB per PDF
- Max pages: provider-dependent (Anthropic allows 100 pages). Client validates and warns if too large.
- Anthropic: native PDF support (passed as-is)
- OpenAI: text extraction on the Rust backend, injected as text

#### API Shape:
```json
POST /v1/sessions/:id/messages
{
  "message": "Summarize this brief",
  "images": [],
  "documents": [
    {
      "data": "<base64>",
      "media_type": "application/pdf",
      "filename": "marketing-brief.pdf"
    }
  ]
}
```

## Technical Design

### Composer Layout
```
┌──────────────────────────────────────────────┐
│ [img1 ✕] [📄 brief.pdf ✕] [📄 data.csv ✕]  │  ← preview strip (visible when attachments queued)
├──────────────────────────────────────────────┤
│ [📎] [  What are we working on?          ] [↑]│  ← attachment button + text field + send
└──────────────────────────────────────────────┘
```

### PendingAttachment Model (Swift)
```swift
struct PendingAttachment: Identifiable {
    let id: UUID
    let kind: AttachmentKind
    let filename: String
    let data: Data
    let mediaType: String
    var thumbnail: PlatformImage?  // NSImage or UIImage

    enum AttachmentKind {
        case image
        case textFile
        case pdf
    }
}
```

### ChatViewModel Changes
- Add `@Published var pendingAttachments: [PendingAttachment] = []`
- `addAttachment(_:)` — validates size/type, generates thumbnail, appends
- `removeAttachment(id:)` — removes by ID
- On send:
  - Images → `[ImagePayload]` (base64 encode)
  - Text files → prepend file contents to message text with `[file: name]` markers
  - PDFs → `[DocumentPayload]` (base64 encode)
  - Pass all to `sendMessageStream()`
  - Clear `pendingAttachments`

### Message Display
- `ContentBlock::Image` → decode base64, display as `AsyncImage` or cached `Image`
- `ContentBlock::Document` → show PDF icon + filename, tap to open in system viewer
- Text file content → already inline in message text, renders normally
- Lazy loading for images; don't decode until visible in scroll view

### Attachment Button Picker (macOS)
```swift
NSOpenPanel:
  allowedContentTypes: [.image, .pdf, .plainText, .json, .yaml, ...]
  allowsMultipleSelection: true
  message: "Attach files to your message"
```

### Attachment Button Picker (iOS)
```swift
Menu with options:
  - "Photo Library" → PHPickerViewController
  - "Camera" → UIImagePickerController (camera source)
  - "Files" → UIDocumentPickerViewController
```

## Testing

### Swift Tests:
- `testImagePayloadEncodingFromData` — raw Data → base64 ImagePayload
- `testPendingAttachmentRemoval` — add 3, remove middle, verify order
- `testMaxAttachmentLimit` — reject 11th attachment with user feedback
- `testImageResizeAboveThreshold` — verify large image gets resized below 5MB
- `testPasteImageFromClipboard` — clipboard image → pending attachment
- `testTextFileReadAndInject` — .csv file → message text with [file:] markers
- `testTextFileRejectsBinary` — binary file rejected with error
- `testTextFileSizeLimit` — >500KB rejected
- `testPDFAttachmentEncoding` — PDF Data → base64 DocumentPayload
- `testPDFSizeLimit` — >10MB rejected

### Backend Tests (Rust):
- Existing image tests (already pass)
- `document_content_block_serializes_for_anthropic` — native PDF block
- `document_content_block_falls_back_to_text_for_openai` — text extraction path
- `documents_field_accepted_in_message_api` — API accepts documents array

## Implementation Order

1. **Swift: PendingAttachment model + ChatViewModel state** — foundation
2. **Swift: Attachment button + NSOpenPanel / PHPicker** — file selection
3. **Swift: Preview strip UI** — thumbnails and remove buttons
4. **Swift: Image send path** — wire pendingAttachments → ImagePayload → API
5. **Swift: Paste + drag-and-drop** — macOS clipboard and drop
6. **Swift: Image display in chat bubbles** — render ContentBlock.image
7. **Swift: Text file injection** — read + prepend to message
8. **Rust: ContentBlock::Document + Anthropic serialization** — backend PDF support
9. **Rust: OpenAI PDF text extraction fallback** — pdf-extract crate
10. **Rust: API accepts documents array** — extend message handler
11. **Swift: PDF send + display** — wire DocumentPayload, render in chat

Steps 1-7 are pure Swift, no backend changes. Steps 8-11 add PDF support across the stack.

## Out of Scope
- TUI attachment support (terminal limitation)
- Video attachments (Phase 3)
- RTF / DOCX (need conversion libraries; future phase)
- Multipart upload (Phase 2; base64 is fine for ≤10MB files)
- Image generation / DALL-E integration (separate feature)
- Server-side file storage (Phase 2; Phase 1 is stateless pass-through)

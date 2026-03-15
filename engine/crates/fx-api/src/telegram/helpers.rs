use crate::types::EncodedImage;
use base64::Engine as _;
use fx_channel_telegram::{IncomingMessage, OutgoingMessage, TelegramChannel};
use fx_core::channel::ResponseContext;
use std::path::{Path, PathBuf};

pub(crate) const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;

pub fn media_inbound_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("media").join("inbound")
}

pub async fn encode_photos(
    telegram: &TelegramChannel,
    incoming: &mut IncomingMessage,
    data_dir: &Path,
) -> Vec<EncodedImage> {
    if incoming.photos.is_empty() {
        return Vec::new();
    }

    let media_dir = media_inbound_dir(data_dir);
    if let Err(error) = std::fs::create_dir_all(&media_dir) {
        tracing::error!("Failed to create media dir: {error}");
        return Vec::new();
    }

    let mut images = Vec::new();
    for photo in &mut incoming.photos {
        let encoded =
            download_and_encode_photo(telegram, incoming.message_id, &media_dir, photo).await;
        if let Some(image) = encoded {
            images.push(image);
        }
    }
    images
}

pub async fn download_and_encode_photo(
    telegram: &TelegramChannel,
    message_id: i64,
    media_dir: &Path,
    photo: &mut fx_channel_telegram::PhotoAttachment,
) -> Option<EncodedImage> {
    let path = download_photo_file(telegram, message_id, media_dir, photo).await?;
    encode_downloaded_photo(photo, &path)
}

async fn download_photo_file(
    telegram: &TelegramChannel,
    message_id: i64,
    media_dir: &Path,
    photo: &mut fx_channel_telegram::PhotoAttachment,
) -> Option<PathBuf> {
    match telegram
        .download_file(&photo.file_id, message_id, media_dir)
        .await
    {
        Ok(path) => {
            photo.file_path = Some(path.clone());
            Some(path)
        }
        Err(error) => {
            tracing::error!("Failed to download photo {}: {error}", photo.file_id);
            None
        }
    }
}

pub(crate) fn encode_downloaded_photo(
    photo: &fx_channel_telegram::PhotoAttachment,
    path: &Path,
) -> Option<EncodedImage> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_IMAGE_BYTES as u64 {
        tracing::warn!(
            path = %path.display(),
            bytes = metadata.len(),
            max_bytes = MAX_IMAGE_BYTES,
            "Skipping oversized Telegram photo"
        );
        return None;
    }

    let media_type = validated_image_mime_type(&photo.mime_type);
    match std::fs::read(path) {
        Ok(bytes) => Some(EncodedImage {
            media_type,
            base64_data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }),
        Err(error) => {
            tracing::error!("Failed to read photo file {}: {error}", path.display());
            None
        }
    }
}

fn validated_image_mime_type(media_type: &str) -> String {
    let trimmed = media_type.trim();
    match trimmed {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" => trimmed.to_string(),
        "" => "image/jpeg".to_string(),
        unexpected => {
            tracing::warn!(
                media_type = unexpected,
                "Unexpected Telegram photo MIME type; defaulting to image/jpeg"
            );
            "image/jpeg".to_string()
        }
    }
}

pub fn telegram_context(incoming: &IncomingMessage) -> ResponseContext {
    let reply_to = if incoming.chat_id < 0 {
        Some(incoming.message_id.to_string())
    } else {
        None
    };
    ResponseContext {
        routing_key: Some(incoming.chat_id.to_string()),
        reply_to,
    }
}

pub fn queue_telegram_error(
    telegram: &TelegramChannel,
    incoming: &IncomingMessage,
    error: &anyhow::Error,
) {
    let context = telegram_context(incoming);
    let message = format!("⚠️ Error: {error}");
    let _ = telegram.queue_response(&message, &context, None);
}

pub async fn flush_telegram_outbound(telegram: &TelegramChannel) {
    for outbound in telegram.drain_outbound() {
        let message = OutgoingMessage {
            chat_id: outbound.chat_id,
            text: outbound.text,
            parse_mode: outbound.parse_mode,
            reply_to_message_id: outbound.reply_to_message_id,
        };
        if let Err(error) = telegram.send_message(&message).await {
            tracing::error!("Telegram send failed: {error}");
            break;
        }
    }
}

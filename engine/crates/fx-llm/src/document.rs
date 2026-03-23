use base64::Engine;

pub(crate) fn document_text_fallback(
    media_type: &str,
    data: &str,
    filename: Option<&str>,
) -> String {
    let filename = filename
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(match media_type {
            "application/pdf" => "attached.pdf",
            _ => "attached-document",
        });

    let extracted = match base64::engine::general_purpose::STANDARD.decode(data.trim()) {
        Ok(bytes) if media_type == "application/pdf" => pdf_extract::extract_text_from_mem(&bytes)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty()),
        _ => None,
    };

    let body = extracted.unwrap_or_else(|| {
        format!(
            "Attached document `{filename}` was provided as `{media_type}`, but text extraction was unavailable."
        )
    });

    format!("[file: {filename}]\n{body}\n[/file: {filename}]")
}

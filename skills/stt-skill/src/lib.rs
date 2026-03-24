use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;

const INFO_LEVEL: u32 = 2;
const ERROR_LEVEL: u32 = 4;
const EMPTY_JSON: &str = "{}";
const OPENAI_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const MULTIPART_BOUNDARY: &str = "----FawxSTTBoundary7MA4YWxkTrZu0gW";
// COUPLING: This sentinel must match the one in
// engine/crates/fx-skills/src/live_host_api.rs.
const HOST_BINARY_BASE64_PREFIX: &str = "__fawx_binary_base64__:";
// COUPLING: This sentinel must match the one in
// engine/crates/fx-skills/src/live_host_api.rs.
const HOST_REQUEST_BINARY_BASE64_PREFIX: &str = "__fawx_request_binary_base64__:";
const MAX_HOST_STRING_LEN: usize = 4_194_304;

#[link(wasm_import_module = "host_api_v1")]
extern "C" {
    #[link_name = "log"]
    fn host_log(level: u32, msg_ptr: *const u8, msg_len: u32);
    #[link_name = "get_input"]
    fn host_get_input() -> u32;
    #[link_name = "set_output"]
    fn host_set_output(text_ptr: *const u8, text_len: u32);
    #[link_name = "kv_get"]
    fn host_kv_get(key_ptr: *const u8, key_len: u32) -> u32;
    #[link_name = "http_request"]
    fn host_http_request(
        method_ptr: *const u8,
        method_len: u32,
        url_ptr: *const u8,
        url_len: u32,
        headers_ptr: *const u8,
        headers_len: u32,
        body_ptr: *const u8,
        body_len: u32,
    ) -> u32;
}

#[derive(Debug, Deserialize)]
struct SttInput {
    audio: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestOptions {
    audio: AudioSource,
    language: Option<String>,
    prompt: Option<String>,
    format: OutputFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AudioSource {
    Url(String),
    Base64(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Verbose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioPayload {
    bytes: Vec<u8>,
    filename: String,
    content_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApiRequest {
    url: String,
    headers: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SttError {
    InvalidInput(String),
    MissingApiKey(String),
    RequestFailed(String),
    ParseFailed(String),
}

#[derive(Debug, Deserialize)]
struct VerboseApiResponse {
    text: Option<String>,
    language: Option<String>,
    duration: Option<f64>,
    segments: Option<Vec<ApiSegment>>,
}

#[derive(Debug, Deserialize)]
struct ApiSegment {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct OutputSegment {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
struct TranscriptionResult {
    text: String,
    language: Option<String>,
    duration: Option<f64>,
    segments: Option<Vec<OutputSegment>>,
}

#[derive(Serialize)]
struct SuccessOutput {
    status: &'static str,
    text: String,
    language: Option<String>,
    duration: Option<f64>,
    segments: Option<Vec<OutputSegment>>,
    message: String,
}

struct HttpRequest<'a> {
    method: &'a str,
    url: &'a str,
    headers: &'a str,
    body: &'a str,
}

trait HostBridge {
    fn kv_get(&self, key: &str) -> Option<String>;
    fn http_request(&self, request: &HttpRequest<'_>) -> Option<String>;
}

struct LiveHostBridge;

impl fmt::Display for SttError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message)
            | Self::MissingApiKey(message)
            | Self::RequestFailed(message)
            | Self::ParseFailed(message) => formatter.write_str(message),
        }
    }
}

impl OutputFormat {
    fn parse(value: Option<&str>) -> Result<Self, SttError> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Text),
            Some("text") => Ok(Self::Text),
            Some("verbose") => Ok(Self::Verbose),
            Some(value) => Err(SttError::InvalidInput(format!(
                "Invalid format '{value}'. Valid formats: text, verbose"
            ))),
        }
    }

    fn api_value(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Verbose => "verbose_json",
        }
    }
}

impl HostBridge for LiveHostBridge {
    fn kv_get(&self, key: &str) -> Option<String> {
        unsafe { read_host_string(host_kv_get(key.as_ptr(), key.len() as u32)) }
    }

    fn http_request(&self, request: &HttpRequest<'_>) -> Option<String> {
        unsafe {
            read_host_string(host_http_request(
                request.method.as_ptr(),
                request.method.len() as u32,
                request.url.as_ptr(),
                request.url.len() as u32,
                request.headers.as_ptr(),
                request.headers.len() as u32,
                request.body.as_ptr(),
                request.body.len() as u32,
            ))
        }
    }
}

/// # Safety
/// `ptr` must be 0 or point to a NUL-terminated string in valid WASM linear memory.
unsafe fn read_host_string(ptr: u32) -> Option<String> {
    if ptr == 0 {
        return None;
    }

    let slice = core::slice::from_raw_parts(ptr as *const u8, MAX_HOST_STRING_LEN);
    let len = slice
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(MAX_HOST_STRING_LEN);
    Some(String::from_utf8_lossy(&slice[..len]).into_owned())
}

fn log(level: u32, message: &str) {
    unsafe {
        host_log(level, message.as_ptr(), message.len() as u32);
    }
}

fn get_input() -> String {
    unsafe { read_host_string(host_get_input()).unwrap_or_default() }
}

fn set_output(text: &str) {
    unsafe {
        host_set_output(text.as_ptr(), text.len() as u32);
    }
}

fn execute(raw_input: &str) -> Result<String, SttError> {
    let host = LiveHostBridge;
    execute_with_host(raw_input, &host)
}

fn execute_with_host(raw_input: &str, host: &impl HostBridge) -> Result<String, SttError> {
    let options = parse_input(raw_input)?;
    let api_key = load_api_key(host.kv_get("openai_api_key"))?;
    let audio = resolve_audio(host, &options.audio)?;
    let request = build_api_request(&options, &api_key, &audio);
    let response = send_request(host, &request)?;
    let result = parse_transcription_response(&response, &options)?;
    Ok(format_success_output(&result))
}

fn parse_input(raw_input: &str) -> Result<RequestOptions, SttError> {
    let input: SttInput = serde_json::from_str(raw_input)
        .map_err(|error| SttError::InvalidInput(format!("Invalid input JSON: {error}")))?;

    Ok(RequestOptions {
        audio: parse_audio_source(input.audio)?,
        language: normalize_optional(input.language),
        prompt: normalize_optional(input.prompt),
        format: OutputFormat::parse(input.format.as_deref())?,
    })
}

fn parse_audio_source(value: Option<String>) -> Result<AudioSource, SttError> {
    let audio = value.unwrap_or_default().trim().to_string();
    if audio.is_empty() {
        return Err(SttError::InvalidInput(
            "Audio input is required".to_string(),
        ));
    }

    if is_audio_url(&audio) {
        return Ok(AudioSource::Url(audio));
    }

    Ok(AudioSource::Base64(audio))
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn is_audio_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn load_api_key(value: Option<String>) -> Result<String, SttError> {
    value
        .map(|api_key| api_key.trim().to_string())
        .filter(|api_key| !api_key.is_empty())
        .ok_or_else(|| {
            SttError::MissingApiKey(
                "No OpenAI API key found. Set 'openai_api_key' in skill storage.".to_string(),
            )
        })
}

fn resolve_audio(host: &impl HostBridge, source: &AudioSource) -> Result<AudioPayload, SttError> {
    match source {
        AudioSource::Url(url) => fetch_audio_payload(host, url),
        AudioSource::Base64(encoded) => decode_audio_payload(encoded),
    }
}

fn fetch_audio_payload(host: &impl HostBridge, url: &str) -> Result<AudioPayload, SttError> {
    let request = HttpRequest {
        method: "GET",
        url,
        headers: EMPTY_JSON,
        body: "",
    };
    let response = host.http_request(&request).ok_or_else(|| {
        SttError::RequestFailed(format!(
            "Transcription failed: failed to fetch audio from {url}"
        ))
    })?;
    let bytes = decode_response_bytes(&response)?;
    Ok(build_audio_payload(bytes, Some(url)))
}

fn decode_audio_payload(encoded: &str) -> Result<AudioPayload, SttError> {
    let bytes = decode_audio_base64(encoded)?;
    Ok(build_audio_payload(bytes, None))
}

fn decode_audio_base64(encoded: &str) -> Result<Vec<u8>, SttError> {
    let encoded = encoded
        .strip_prefix(HOST_BINARY_BASE64_PREFIX)
        .unwrap_or(encoded);
    decode_base64_bytes(encoded).map_err(|_| invalid_base64_error())
}

fn decode_response_bytes(response: &str) -> Result<Vec<u8>, SttError> {
    match response.strip_prefix(HOST_BINARY_BASE64_PREFIX) {
        Some(encoded) => decode_base64_bytes(encoded).map_err(|_| {
            SttError::RequestFailed(
                "Transcription failed: invalid binary response from host".to_string(),
            )
        }),
        None => Ok(response.as_bytes().to_vec()),
    }
}

fn invalid_base64_error() -> SttError {
    SttError::InvalidInput("Failed to decode audio: invalid base64 encoding".to_string())
}

fn build_audio_payload(bytes: Vec<u8>, source_url: Option<&str>) -> AudioPayload {
    let extension = infer_audio_extension(&bytes)
        .or_else(|| extension_from_url(source_url))
        .unwrap_or("mp3");

    AudioPayload {
        bytes,
        filename: format!("audio.{extension}"),
        content_type: content_type_for_extension(extension),
    }
}

fn infer_audio_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"ID3") || is_mpeg_frame(bytes) {
        return Some("mp3");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        return Some("wav");
    }
    if bytes.starts_with(b"OggS") {
        return Some("ogg");
    }
    if bytes.starts_with(b"fLaC") {
        return Some("flac");
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("webm");
    }
    if bytes.get(4..8) == Some(b"ftyp") {
        return Some("m4a");
    }
    None
}

fn is_mpeg_frame(bytes: &[u8]) -> bool {
    matches!(bytes, [0xFF, second, ..] if second & 0xE0 == 0xE0)
}

fn extension_from_url(url: Option<&str>) -> Option<&'static str> {
    let path = url?.split('?').next().unwrap_or_default();
    let extension = path.rsplit('.').next()?.to_ascii_lowercase();
    match extension.as_str() {
        "mp3" => Some("mp3"),
        "wav" => Some("wav"),
        "ogg" => Some("ogg"),
        "flac" => Some("flac"),
        "webm" => Some("webm"),
        "m4a" | "mp4" | "mpeg" | "mpga" => Some("m4a"),
        _ => None,
    }
}

fn content_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "webm" => "audio/webm",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

fn build_api_request(options: &RequestOptions, api_key: &str, audio: &AudioPayload) -> ApiRequest {
    let headers = json!({
        "Authorization": format!("Bearer {api_key}"),
        "Content-Type": format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}")
    })
    .to_string();

    ApiRequest {
        url: OPENAI_URL.to_string(),
        headers,
        body: encode_request_body(&build_multipart_body(options, audio)),
    }
}

fn build_multipart_body(options: &RequestOptions, audio: &AudioPayload) -> Vec<u8> {
    let mut body = Vec::new();
    append_text_part(&mut body, "model", "whisper-1");
    append_text_part(&mut body, "response_format", options.format.api_value());

    if let Some(language) = &options.language {
        append_text_part(&mut body, "language", language);
    }
    if let Some(prompt) = &options.prompt {
        append_text_part(&mut body, "prompt", prompt);
    }

    append_file_part(&mut body, audio);
    append_closing_boundary(&mut body);
    body
}

fn append_text_part(body: &mut Vec<u8>, name: &str, value: &str) {
    let part = format!(
        "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
    );
    body.extend_from_slice(part.as_bytes());
}

fn append_file_part(body: &mut Vec<u8>, audio: &AudioPayload) {
    let header = format!(
        "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
        audio.filename, audio.content_type
    );
    body.extend_from_slice(header.as_bytes());
    body.extend_from_slice(&audio.bytes);
    body.extend_from_slice(b"\r\n");
}

fn append_closing_boundary(body: &mut Vec<u8>) {
    let closing = format!("--{MULTIPART_BOUNDARY}--\r\n");
    body.extend_from_slice(closing.as_bytes());
}

fn encode_request_body(body: &[u8]) -> String {
    format!("{HOST_REQUEST_BINARY_BASE64_PREFIX}{}", base64_encode(body))
}

fn send_request(host: &impl HostBridge, request: &ApiRequest) -> Result<String, SttError> {
    let call = HttpRequest {
        method: "POST",
        url: &request.url,
        headers: &request.headers,
        body: &request.body,
    };

    host.http_request(&call)
        .filter(|response| !response.is_empty())
        .ok_or_else(|| SttError::RequestFailed("Transcription failed: request failed".to_string()))
}

fn parse_transcription_response(
    response: &str,
    options: &RequestOptions,
) -> Result<TranscriptionResult, SttError> {
    if let Some(message) = extract_api_error(response) {
        return Err(SttError::RequestFailed(format!(
            "Transcription failed: {message}"
        )));
    }

    match options.format {
        OutputFormat::Text => Ok(TranscriptionResult {
            text: response.to_string(),
            language: options.language.clone(),
            duration: None,
            segments: None,
        }),
        OutputFormat::Verbose => parse_verbose_response(response, options),
    }
}

fn parse_verbose_response(
    response: &str,
    options: &RequestOptions,
) -> Result<TranscriptionResult, SttError> {
    let parsed: VerboseApiResponse = serde_json::from_str(response).map_err(|error| {
        SttError::ParseFailed(format!("Failed to parse transcription response: {error}"))
    })?;

    Ok(TranscriptionResult {
        text: parsed.text.unwrap_or_default(),
        language: parsed.language.or_else(|| options.language.clone()),
        duration: parsed.duration,
        segments: parsed.segments.map(map_segments),
    })
}

fn map_segments(segments: Vec<ApiSegment>) -> Vec<OutputSegment> {
    segments
        .into_iter()
        .map(|segment| OutputSegment {
            start: segment.start,
            end: segment.end,
            text: segment.text,
        })
        .collect()
}

fn extract_api_error(response: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(response).ok()?;
    value.get("error").and_then(json_error_message).or_else(|| {
        value
            .get("message")
            .and_then(|value| value.as_str())
            .map(str::to_string)
    })
}

fn json_error_message(value: &serde_json::Value) -> Option<String> {
    value
        .get("message")
        .and_then(|message| message.as_str())
        .map(str::to_string)
        .or_else(|| value.as_str().map(str::to_string))
}

fn format_success_output(result: &TranscriptionResult) -> String {
    serialize_json(&SuccessOutput {
        status: "success",
        text: result.text.clone(),
        language: result.language.clone(),
        duration: result.duration,
        segments: result.segments.clone(),
        message: format_success_message(result),
    })
}

fn format_success_message(result: &TranscriptionResult) -> String {
    let mut parts = vec![format!("{} chars", result.text.chars().count())];

    if let Some(duration) = result.duration {
        parts.push(format!("{}s", format_decimal(duration)));
    }
    if let Some(language) = &result.language {
        parts.push(format!("language: {language}"));
    }
    if let Some(segments) = &result.segments {
        parts.push(format!("{} segments", segments.len()));
    }

    format!("🎤 Transcribed audio ({})", parts.join(", "))
}

fn format_decimal(value: f64) -> String {
    let rendered = format!("{value:.1}");
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn error_output(error: &SttError) -> String {
    serialize_json(&json!({ "error": error.to_string() }))
}

fn serialize_json<T: Serialize>(value: &T) -> String {
    match serde_json::to_string(value) {
        Ok(serialized) => serialized,
        Err(_) => r#"{"error":"Internal serialization error."}"#.to_string(),
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let combined = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

        output.push(TABLE[((combined >> 18) & 0x3F) as usize] as char);
        output.push(TABLE[((combined >> 12) & 0x3F) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((combined >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(combined & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn decode_base64_bytes(input: &str) -> Result<Vec<u8>, ()> {
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();

    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err(());
    }

    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        decode_base64_chunk(chunk, &mut output)?;
    }
    Ok(output)
}

fn decode_base64_chunk(chunk: &[u8], output: &mut Vec<u8>) -> Result<(), ()> {
    let v0 = decode_base64_value(chunk[0])?;
    let v1 = decode_base64_value(chunk[1])?;
    let v2 = decode_optional_base64_value(chunk[2])?;
    let v3 = decode_optional_base64_value(chunk[3])?;
    let combined = ((v0 as u32) << 18)
        | ((v1 as u32) << 12)
        | ((v2.unwrap_or(0) as u32) << 6)
        | v3.unwrap_or(0) as u32;

    output.push(((combined >> 16) & 0xFF) as u8);
    if v2.is_some() {
        output.push(((combined >> 8) & 0xFF) as u8);
    }
    if v3.is_some() {
        output.push((combined & 0xFF) as u8);
    }
    Ok(())
}

fn decode_optional_base64_value(byte: u8) -> Result<Option<u8>, ()> {
    if byte == b'=' {
        return Ok(None);
    }
    decode_base64_value(byte).map(Some)
}

fn decode_base64_value(byte: u8) -> Result<u8, ()> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(()),
    }
}

#[no_mangle]
pub extern "C" fn run() {
    log(INFO_LEVEL, "STT skill starting");
    let input = get_input();

    match execute(&input) {
        Ok(output) => set_output(&output),
        Err(error) => {
            log(ERROR_LEVEL, &error.to_string());
            set_output(&error_output(&error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeHost {
        storage: HashMap<String, String>,
        responses: HashMap<String, String>,
        requests: RefCell<Vec<LoggedRequest>>,
    }

    #[derive(Clone)]
    struct LoggedRequest {
        method: String,
        url: String,
        headers: String,
        body: String,
    }

    impl FakeHost {
        fn with_storage(mut self, key: &str, value: &str) -> Self {
            self.storage.insert(key.to_string(), value.to_string());
            self
        }

        fn with_response(mut self, url: &str, body: &str) -> Self {
            self.responses.insert(url.to_string(), body.to_string());
            self
        }
    }

    impl HostBridge for FakeHost {
        fn kv_get(&self, key: &str) -> Option<String> {
            self.storage.get(key).cloned()
        }

        fn http_request(&self, request: &HttpRequest<'_>) -> Option<String> {
            self.requests.borrow_mut().push(LoggedRequest {
                method: request.method.to_string(),
                url: request.url.to_string(),
                headers: request.headers.to_string(),
                body: request.body.to_string(),
            });
            self.responses.get(request.url).cloned()
        }
    }

    fn sample_options() -> RequestOptions {
        RequestOptions {
            audio: AudioSource::Base64("AQID".to_string()),
            language: Some("en".to_string()),
            prompt: Some("RustConf".to_string()),
            format: OutputFormat::Verbose,
        }
    }

    #[test]
    fn parse_input_accepts_all_parameters() {
        let options = parse_input(
            r#"{"audio":"AQID","language":"en","prompt":"RustConf","format":"verbose"}"#,
        )
        .expect("input should parse");

        assert_eq!(options.audio, AudioSource::Base64("AQID".to_string()));
        assert_eq!(options.language, Some("en".to_string()));
        assert_eq!(options.prompt, Some("RustConf".to_string()));
        assert_eq!(options.format, OutputFormat::Verbose);
    }

    #[test]
    fn parse_input_uses_defaults() {
        let options = parse_input(r#"{"audio":"AQID"}"#).expect("input should parse");

        assert_eq!(options.audio, AudioSource::Base64("AQID".to_string()));
        assert_eq!(options.language, None);
        assert_eq!(options.prompt, None);
        assert_eq!(options.format, OutputFormat::Text);
    }

    #[test]
    fn missing_audio_returns_friendly_error() {
        let error = parse_input(r#"{"format":"text"}"#).expect_err("audio should be required");
        assert_eq!(error.to_string(), "Audio input is required");
    }

    #[test]
    fn missing_api_key_returns_friendly_error() {
        let error = execute_with_host(r#"{"audio":"AQID"}"#, &FakeHost::default())
            .expect_err("api key should be required");
        assert_eq!(
            error.to_string(),
            "No OpenAI API key found. Set 'openai_api_key' in skill storage."
        );
    }

    #[test]
    fn invalid_base64_returns_friendly_error() {
        let host = FakeHost::default().with_storage("openai_api_key", "secret");
        let error =
            execute_with_host(r#"{"audio":"%%%"}"#, &host).expect_err("base64 should be rejected");
        assert_eq!(
            error.to_string(),
            "Failed to decode audio: invalid base64 encoding"
        );
    }

    #[test]
    fn invalid_format_returns_friendly_error() {
        let error =
            parse_input(r#"{"audio":"AQID","format":"json"}"#).expect_err("format should fail");
        assert_eq!(
            error.to_string(),
            "Invalid format 'json'. Valid formats: text, verbose"
        );
    }

    #[test]
    fn url_audio_detection_accepts_http_and_https() {
        for url in [
            "http://example.com/audio.mp3",
            "https://example.com/audio.mp3",
        ] {
            let source = parse_audio_source(Some(url.to_string())).expect("url should parse");
            assert_eq!(source, AudioSource::Url(url.to_string()));
        }
    }

    #[test]
    fn base64_audio_detection_uses_non_url_input() {
        let source = parse_audio_source(Some("AQID".to_string())).expect("base64 should parse");
        assert_eq!(source, AudioSource::Base64("AQID".to_string()));
    }

    #[test]
    fn multipart_body_contains_boundary_fields_and_file_bytes() {
        let request = build_api_request(
            &sample_options(),
            "secret",
            &AudioPayload {
                bytes: vec![1, 2, 3],
                filename: "audio.mp3".to_string(),
                content_type: "audio/mpeg",
            },
        );
        let encoded = request
            .body
            .strip_prefix(HOST_REQUEST_BINARY_BASE64_PREFIX)
            .expect("prefix");
        let decoded = decode_base64_bytes(encoded).expect("body should decode");
        let multipart = String::from_utf8_lossy(&decoded);

        assert!(request.headers.contains(MULTIPART_BOUNDARY));
        assert!(multipart.contains("name=\"model\"\r\n\r\nwhisper-1"));
        assert!(multipart.contains("name=\"response_format\"\r\n\r\nverbose_json"));
        assert!(multipart.contains("name=\"language\"\r\n\r\nen"));
        assert!(multipart.contains("name=\"prompt\"\r\n\r\nRustConf"));
        assert!(multipart.contains("name=\"file\"; filename=\"audio.mp3\""));
        assert!(multipart.contains("Content-Type: audio/mpeg"));
        assert!(decoded.windows(3).any(|window| window == [1, 2, 3]));
        assert!(multipart.ends_with(&format!("--{MULTIPART_BOUNDARY}--\r\n")));
    }

    #[test]
    fn parse_text_response_uses_requested_language() {
        let options = RequestOptions {
            audio: AudioSource::Base64("AQID".to_string()),
            language: Some("en".to_string()),
            prompt: None,
            format: OutputFormat::Text,
        };
        let result = parse_transcription_response("Hello world", &options)
            .expect("text response should parse");

        assert_eq!(result.text, "Hello world");
        assert_eq!(result.language, Some("en".to_string()));
        assert_eq!(result.duration, None);
        assert_eq!(result.segments, None);
    }

    #[test]
    fn parse_verbose_response_includes_segments() {
        let response = r#"{"text":"Hello, this is a test.","language":"en","duration":3.5,"segments":[{"start":0.0,"end":1.2,"text":"Hello,"},{"start":1.2,"end":3.5,"text":"this is a test."}]}"#;
        let result = parse_transcription_response(response, &sample_options())
            .expect("verbose response should parse");

        assert_eq!(result.text, "Hello, this is a test.");
        assert_eq!(result.language, Some("en".to_string()));
        assert_eq!(result.duration, Some(3.5));
        assert_eq!(result.segments.as_ref().map(Vec::len), Some(2));
        assert_eq!(result.segments.expect("segments")[0].text, "Hello,");
    }

    #[test]
    fn output_message_formats_all_available_metadata() {
        let message = format_success_message(&TranscriptionResult {
            text: "Hello, this is a test.".to_string(),
            language: Some("en".to_string()),
            duration: Some(3.5),
            segments: Some(vec![
                OutputSegment {
                    start: 0.0,
                    end: 1.2,
                    text: "Hello,".to_string(),
                },
                OutputSegment {
                    start: 1.2,
                    end: 3.5,
                    text: "this is a test.".to_string(),
                },
            ]),
        });

        assert_eq!(
            message,
            "🎤 Transcribed audio (22 chars, 3.5s, language: en, 2 segments)"
        );
    }

    #[test]
    fn base64_sentinel_prefix_is_stripped_before_decode() {
        let decoded = decode_audio_base64(&format!("{HOST_BINARY_BASE64_PREFIX}AQID"))
            .expect("prefixed base64 should decode");
        assert_eq!(decoded, vec![1, 2, 3]);
    }

    #[test]
    fn url_audio_fetch_uses_get_and_posts_transcription_request() {
        let verbose = r#"{"text":"Hello","language":"en","duration":1.0,"segments":[]}"#;
        let host = FakeHost::default()
            .with_storage("openai_api_key", "secret")
            .with_response(
                "https://example.com/audio.mp3",
                &format!("{HOST_BINARY_BASE64_PREFIX}AQID"),
            )
            .with_response(OPENAI_URL, verbose);

        let output = execute_with_host(
            r#"{"audio":"https://example.com/audio.mp3","format":"verbose"}"#,
            &host,
        )
        .expect("url flow should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("json output");
        let requests = host.requests.borrow();

        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].url, "https://example.com/audio.mp3");
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].url, OPENAI_URL);
        assert!(requests[1].headers.contains(MULTIPART_BOUNDARY));
        assert!(requests[1]
            .body
            .starts_with(HOST_REQUEST_BINARY_BASE64_PREFIX));
        assert_eq!(parsed["status"], "success");
        assert_eq!(parsed["text"], "Hello");
    }
}

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;

const INFO_LEVEL: u32 = 2;
const ERROR_LEVEL: u32 = 4;
const DEFAULT_SPEED: &str = "1.0";
const OPENAI_URL: &str = "https://api.openai.com/v1/audio/speech";
const MAX_HOST_STRING_LEN: usize = 262_144;
/// Prefix for binary HTTP responses bridged over the string-only WASM ABI.
///
/// The host prepends this sentinel before base64-encoding raw bytes. Collision
/// with a real text response is extremely unlikely because this prefix is not
/// valid JSON, HTML, or any known API response format.
// COUPLING: This sentinel must match the one in
// engine/crates/fx-skills/src/live_host_api.rs.
const HOST_BINARY_BASE64_PREFIX: &str = "__fawx_binary_base64__:";
const VALID_VOICES: [&str; 6] = ["alloy", "echo", "fable", "onyx", "nova", "shimmer"];

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
struct TtsInput {
    text: Option<String>,
    voice: Option<String>,
    provider: Option<String>,
    speed: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct RequestOptions {
    text: String,
    voice: Voice,
    provider: Provider,
    speed: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    OpenAi,
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Voice {
    Alloy,
    Echo,
    Fable,
    Onyx,
    Nova,
    Shimmer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApiRequest {
    url: String,
    headers: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TtsError {
    InvalidInput(String),
    MissingApiKey(String),
    RequestFailed(String),
}

#[derive(Serialize)]
struct SuccessOutput<'a> {
    status: &'a str,
    provider: &'a str,
    voice: &'a str,
    format: &'a str,
    audio_base64: &'a str,
    text_length: usize,
    message: String,
}

struct HttpRequest<'a> {
    method: &'a str,
    url: &'a str,
    headers: &'a str,
    body: &'a str,
}

impl fmt::Display for TtsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message)
            | Self::MissingApiKey(message)
            | Self::RequestFailed(message) => formatter.write_str(message),
        }
    }
}

impl Provider {
    fn parse(value: Option<&str>) -> Result<Self, TtsError> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::OpenAi),
            Some(value) if value.eq_ignore_ascii_case("openai") => Ok(Self::OpenAi),
            Some(value) if value.eq_ignore_ascii_case("edge") => Ok(Self::Edge),
            Some(value) => Err(TtsError::InvalidInput(format!(
                "Invalid provider '{value}'. Use 'openai' or 'edge'."
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Edge => "edge",
        }
    }
}

impl Voice {
    fn parse(value: Option<&str>) -> Result<Self, TtsError> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Alloy),
            Some("alloy") => Ok(Self::Alloy),
            Some("echo") => Ok(Self::Echo),
            Some("fable") => Ok(Self::Fable),
            Some("onyx") => Ok(Self::Onyx),
            Some("nova") => Ok(Self::Nova),
            Some("shimmer") => Ok(Self::Shimmer),
            Some(value) => Err(invalid_voice_error(value)),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Alloy => "alloy",
            Self::Echo => "echo",
            Self::Fable => "fable",
            Self::Onyx => "onyx",
            Self::Nova => "nova",
            Self::Shimmer => "shimmer",
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

fn kv_get(key: &str) -> Option<String> {
    unsafe { read_host_string(host_kv_get(key.as_ptr(), key.len() as u32)) }
}

fn http_request(request: &HttpRequest<'_>) -> Option<String> {
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

fn execute(raw_input: &str) -> Result<String, TtsError> {
    let options = parse_input(raw_input)?;
    match options.provider {
        Provider::OpenAi => execute_openai(&options),
        Provider::Edge => Err(TtsError::InvalidInput(edge_provider_message().to_string())),
    }
}

fn execute_openai(options: &RequestOptions) -> Result<String, TtsError> {
    let api_key = get_api_key()?;
    let request = build_openai_request(options, &api_key);
    let response = send_request(&request)?;
    ensure_successful_response(&response)?;
    let audio_base64 = response_audio_base64(&response);
    Ok(format_success_output(options, &audio_base64))
}

fn parse_input(raw_input: &str) -> Result<RequestOptions, TtsError> {
    let input: TtsInput = serde_json::from_str(raw_input)
        .map_err(|error| TtsError::InvalidInput(format!("Invalid input JSON: {error}")))?;

    Ok(RequestOptions {
        text: parse_text(input.text)?,
        voice: Voice::parse(input.voice.as_deref())?,
        provider: Provider::parse(input.provider.as_deref())?,
        speed: parse_speed(input.speed.as_deref())?,
    })
}

fn parse_text(text: Option<String>) -> Result<String, TtsError> {
    let text = text.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err(TtsError::InvalidInput(
            "Text is required for speech generation.".to_string(),
        ));
    }
    Ok(text)
}

fn parse_speed(speed: Option<&str>) -> Result<f32, TtsError> {
    let speed = speed.unwrap_or(DEFAULT_SPEED).trim();
    let parsed = speed.parse::<f32>().map_err(|_| invalid_speed_error())?;

    if !(0.25..=4.0).contains(&parsed) {
        return Err(invalid_speed_error());
    }

    Ok(parsed)
}

fn invalid_voice_error(voice: &str) -> TtsError {
    TtsError::InvalidInput(format!(
        "Invalid voice '{voice}'. Valid voices: {}",
        VALID_VOICES.join(", ")
    ))
}

fn invalid_speed_error() -> TtsError {
    TtsError::InvalidInput("Speed must be between 0.25 and 4.0".to_string())
}

fn edge_provider_message() -> &'static str {
    "Edge TTS requires WebSocket support which is not yet available in WASM skills. Use 'openai' provider instead."
}

fn get_api_key() -> Result<String, TtsError> {
    load_api_key(kv_get("openai_api_key"))
}

fn load_api_key(value: Option<String>) -> Result<String, TtsError> {
    value
        .map(|api_key| api_key.trim().to_string())
        .filter(|api_key| !api_key.is_empty())
        .ok_or_else(|| {
            TtsError::MissingApiKey(
                "No OpenAI API key found. Set 'openai_api_key' in skill storage.".to_string(),
            )
        })
}

fn build_openai_request(options: &RequestOptions, api_key: &str) -> ApiRequest {
    let headers = json!({
        "Authorization": format!("Bearer {api_key}"),
        "Content-Type": "application/json"
    })
    .to_string();

    let body = json!({
        "model": "tts-1",
        "input": options.text,
        "voice": options.voice.as_str(),
        "speed": options.speed,
        "response_format": "mp3"
    })
    .to_string();

    ApiRequest {
        url: OPENAI_URL.to_string(),
        headers,
        body,
    }
}

fn send_request(request: &ApiRequest) -> Result<String, TtsError> {
    let call = HttpRequest {
        method: "POST",
        url: &request.url,
        headers: &request.headers,
        body: &request.body,
    };

    http_request(&call)
        .filter(|response| !response.is_empty())
        .ok_or_else(|| TtsError::RequestFailed("TTS API request failed".to_string()))
}

fn ensure_successful_response(response: &str) -> Result<(), TtsError> {
    if is_error_response(response) {
        return Err(TtsError::RequestFailed(
            "TTS API request failed".to_string(),
        ));
    }
    Ok(())
}

fn is_error_response(response: &str) -> bool {
    // This only detects JSON-formatted error payloads, which is the contract
    // OpenAI TTS uses. Non-JSON or binary failures must be surfaced by the host
    // via HTTP status handling before the skill treats the response as audio.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(response) else {
        return false;
    };

    value.get("error").is_some()
}

fn response_audio_base64(response: &str) -> String {
    match response.strip_prefix(HOST_BINARY_BASE64_PREFIX) {
        Some(encoded) => encoded.to_string(),
        None => base64_encode(response.as_bytes()),
    }
}

// COUPLING: This encoder must match the one in
// engine/crates/fx-skills/src/live_host_api.rs.
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

fn format_success_output(options: &RequestOptions, audio_base64: &str) -> String {
    serialize_json(&SuccessOutput {
        status: "success",
        provider: options.provider.as_str(),
        voice: options.voice.as_str(),
        format: "mp3",
        audio_base64,
        text_length: options.text.chars().count(),
        message: format_success_message(options),
    })
}

fn format_success_message(options: &RequestOptions) -> String {
    format!(
        "🔊 Generated speech ({} chars, voice: {}, OpenAI TTS)",
        options.text.chars().count(),
        options.voice.as_str()
    )
}

fn error_output(error: &TtsError) -> String {
    serialize_json(&json!({ "error": error.to_string() }))
}

fn serialize_json<T: Serialize>(value: &T) -> String {
    match serde_json::to_string(value) {
        Ok(serialized) => serialized,
        Err(_) => r#"{"error":"Internal serialization error."}"#.to_string(),
    }
}

#[no_mangle]
pub extern "C" fn run() {
    log(INFO_LEVEL, "TTS skill starting");
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

    fn sample_options() -> RequestOptions {
        RequestOptions {
            text: "Hello from Fawx".to_string(),
            voice: Voice::Alloy,
            provider: Provider::OpenAi,
            speed: 1.25,
        }
    }

    #[test]
    fn parse_input_accepts_all_parameters() {
        let options =
            parse_input(r#"{"text":"Hello","voice":"nova","provider":"edge","speed":"1.5"}"#)
                .expect("input should parse");

        assert_eq!(options.text, "Hello");
        assert_eq!(options.voice, Voice::Nova);
        assert_eq!(options.provider, Provider::Edge);
        assert!((options.speed - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_input_uses_defaults() {
        let options = parse_input(r#"{"text":"Hello"}"#).expect("input should parse");

        assert_eq!(options.text, "Hello");
        assert_eq!(options.voice, Voice::Alloy);
        assert_eq!(options.provider, Provider::OpenAi);
        assert!((options.speed - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn voice_validation_accepts_supported_values() {
        for voice in VALID_VOICES {
            assert!(
                Voice::parse(Some(voice)).is_ok(),
                "voice should parse: {voice}"
            );
        }
    }

    #[test]
    fn voice_validation_rejects_unknown_voice() {
        let error = Voice::parse(Some("robot")).expect_err("voice should fail");
        assert_eq!(
            error.to_string(),
            "Invalid voice 'robot'. Valid voices: alloy, echo, fable, onyx, nova, shimmer"
        );
    }

    #[test]
    fn speed_validation_accepts_supported_range() {
        for speed in ["0.25", "1.0", "4.0"] {
            assert!(
                parse_speed(Some(speed)).is_ok(),
                "speed should parse: {speed}"
            );
        }
    }

    #[test]
    fn speed_validation_rejects_invalid_values() {
        for speed in ["0.24", "4.01", "fast"] {
            let error = parse_speed(Some(speed)).expect_err("speed should fail");
            assert_eq!(error.to_string(), "Speed must be between 0.25 and 4.0");
        }
    }

    #[test]
    fn empty_text_returns_friendly_error() {
        let error = parse_input(r#"{"text":"   "}"#).expect_err("text should be required");
        assert_eq!(error.to_string(), "Text is required for speech generation.");
    }

    #[test]
    fn missing_api_key_returns_friendly_error() {
        let error = load_api_key(None).expect_err("api key should be required");
        assert_eq!(
            error.to_string(),
            "No OpenAI API key found. Set 'openai_api_key' in skill storage."
        );
    }

    #[test]
    fn edge_provider_returns_limitation_message() {
        let options =
            parse_input(r#"{"text":"Hello","provider":"edge"}"#).expect("input should parse");
        let result = match options.provider {
            Provider::Edge => Err(TtsError::InvalidInput(edge_provider_message().to_string())),
            Provider::OpenAi => Ok(String::new()),
        };

        assert_eq!(
            result.expect_err("edge should be unsupported").to_string(),
            edge_provider_message()
        );
    }

    #[test]
    fn openai_request_body_matches_contract() {
        let request = build_openai_request(&sample_options(), "secret");
        let headers: serde_json::Value = serde_json::from_str(&request.headers).expect("headers");
        let body: serde_json::Value = serde_json::from_str(&request.body).expect("body");

        assert_eq!(request.url, OPENAI_URL);
        assert_eq!(headers["Authorization"], "Bearer secret");
        assert_eq!(body["model"], "tts-1");
        assert_eq!(body["input"], "Hello from Fawx");
        assert_eq!(body["voice"], "alloy");
        assert_eq!(body["response_format"], "mp3");
        assert_eq!(body["speed"], 1.25);
    }

    #[test]
    fn response_formatting_matches_contract() {
        let output = format_success_output(&sample_options(), "aGVsbG8=");
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["status"], "success");
        assert_eq!(parsed["provider"], "openai");
        assert_eq!(parsed["voice"], "alloy");
        assert_eq!(parsed["format"], "mp3");
        assert_eq!(parsed["audio_base64"], "aGVsbG8=");
        assert_eq!(parsed["text_length"], 15);
        assert_eq!(
            parsed["message"],
            "🔊 Generated speech (15 chars, voice: alloy, OpenAI TTS)"
        );
    }

    #[test]
    fn response_audio_base64_encodes_plain_text_bytes() {
        assert_eq!(response_audio_base64("hi"), "aGk=");
    }

    #[test]
    fn response_audio_base64_uses_host_binary_payloads() {
        let response = format!("{HOST_BINARY_BASE64_PREFIX}AQID");
        assert_eq!(response_audio_base64(&response), "AQID");
    }

    #[test]
    fn request_failure_detects_error_json() {
        let error = ensure_successful_response(r#"{"error":{"message":"bad request"}}"#)
            .expect_err("error expected");
        assert_eq!(error.to_string(), "TTS API request failed");
    }

    #[test]
    fn error_output_uses_json_contract() {
        let output = error_output(&TtsError::RequestFailed(
            "TTS API request failed".to_string(),
        ));
        assert_eq!(output, r#"{"error":"TTS API request failed"}"#);
    }

    #[test]
    fn manifest_declares_expected_tool_and_capabilities() {
        let manifest = include_str!("../manifest.toml");
        assert!(manifest.contains(r#"capabilities = ["network", "storage"]"#));
        assert!(manifest.contains(r#"name = "text_to_speech""#));
    }

    #[test]
    fn cargo_manifest_matches_release_profile_contract() {
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains("strip = true"));
        assert!(manifest.contains("crate-type = [\"cdylib\"]"));
    }
}

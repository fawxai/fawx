//! Weather skill - Fetches weather information for a location.
//!
//! This skill demonstrates network capability usage.
//! In a production implementation, this would call the Open-Meteo API.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct WeatherQuery {
    location: String,
}

#[derive(Serialize, Deserialize)]
struct WeatherResponse {
    location: String,
    temperature: f64,
    condition: String,
}

/// Host API imports — linked to the "host_api_v1" WASM import module.
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
    #[link_name = "kv_set"]
    fn host_kv_set(key_ptr: *const u8, key_len: u32, val_ptr: *const u8, val_len: u32);
}

/// Maximum string length to read from host memory.
const MAX_HOST_STRING_LEN: usize = 4096;

/// Read a null-terminated string from a pointer in WASM linear memory.
///
/// The host writes null-terminated strings via `write_to_memory`.
/// This function reads up to `MAX_HOST_STRING_LEN` bytes or until a null byte.
///
/// # Safety
/// The caller must ensure `ptr` points to valid WASM linear memory.
unsafe fn read_host_string(ptr: u32) -> String {
    if ptr == 0 {
        return String::new();
    }

    let slice = core::slice::from_raw_parts(ptr as *const u8, MAX_HOST_STRING_LEN);

    // Find null terminator
    let len = slice.iter().position(|&b| b == 0).unwrap_or(MAX_HOST_STRING_LEN);

    String::from_utf8_lossy(&slice[..len]).to_string()
}

/// Log a message
fn log(level: u32, message: &str) {
    unsafe {
        host_log(level, message.as_ptr(), message.len() as u32);
    }
}

/// Get input from host
fn get_input() -> String {
    unsafe {
        let ptr = host_get_input();
        read_host_string(ptr)
    }
}

/// Set output to host
fn set_output(text: &str) {
    unsafe {
        host_set_output(text.as_ptr(), text.len() as u32);
    }
}

/// Get value from key-value storage
fn kv_get(key: &str) -> Option<String> {
    unsafe {
        let ptr = host_kv_get(key.as_ptr(), key.len() as u32);
        if ptr == 0 {
            None
        } else {
            Some(read_host_string(ptr))
        }
    }
}

/// Set value in key-value storage
fn kv_set(key: &str, value: &str) {
    unsafe {
        host_kv_set(
            key.as_ptr(),
            key.len() as u32,
            value.as_ptr(),
            value.len() as u32,
        );
    }
}

/// Skill entry point
#[no_mangle]
pub extern "C" fn run() {
    log(2, "Weather skill starting");

    // Get input
    let input = get_input();
    log(2, &format!("Received input: {}", input));

    // Parse input
    let query: WeatherQuery = match serde_json::from_str(&input) {
        Ok(q) => q,
        Err(e) => {
            let error_msg = format!("Failed to parse input: {}", e);
            log(4, &error_msg);
            set_output(&format!(
                "{{\"error\": \"Invalid input format. Expected JSON with 'location' field.\"}}"
            ));
            return;
        }
    };

    log(2, &format!("Looking up weather for: {}", query.location));

    // Check cache first
    let cache_key = format!("weather:{}", query.location);
    if let Some(cached) = kv_get(&cache_key) {
        log(2, "Returning cached weather data");
        set_output(&cached);
        return;
    }

    // In a real implementation, this would call the Open-Meteo API
    // For now, we return mock data
    let response = WeatherResponse {
        location: query.location.clone(),
        temperature: 22.5,
        condition: "Sunny".to_string(),
    };

    let response_json = match serde_json::to_string(&response) {
        Ok(json) => json,
        Err(e) => {
            let error_msg = format!("Failed to serialize response: {}", e);
            log(4, &error_msg);
            set_output(&format!("{{\"error\": \"{}\"}}", error_msg));
            return;
        }
    };

    // Cache the result
    kv_set(&cache_key, &response_json);

    log(2, "Weather lookup complete");
    set_output(&response_json);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weather_query_parse() {
        let json = r#"{"location": "San Francisco"}"#;
        let query: WeatherQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.location, "San Francisco");
    }

    #[test]
    fn test_weather_response_serialize() {
        let response = WeatherResponse {
            location: "Tokyo".to_string(),
            temperature: 18.5,
            condition: "Cloudy".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Tokyo"));
        assert!(json.contains("18.5"));
    }
}

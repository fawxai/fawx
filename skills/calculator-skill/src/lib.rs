//! Calculator skill - Evaluates simple mathematical expressions.
//!
//! This skill demonstrates basic computation without requiring external capabilities.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct CalculatorQuery {
    expression: String,
}

#[derive(Serialize, Deserialize)]
struct CalculatorResponse {
    result: f64,
    expression: String,
}

// Host API imports — linked to the "host_api_v1" WASM import module.
#[link(wasm_import_module = "host_api_v1")]
extern "C" {
    #[link_name = "log"]
    fn host_log(level: u32, msg_ptr: *const u8, msg_len: u32);
    #[link_name = "get_input"]
    fn host_get_input() -> u32;
    #[link_name = "set_output"]
    fn host_set_output(text_ptr: *const u8, text_len: u32);
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

/// Evaluate a simple mathematical expression
/// Supports: +, -, *, /
/// Example: "2 + 3 * 4" -> 14.0
fn evaluate(expr: &str) -> Result<f64, String> {
    let expr = expr.trim().replace(' ', "");
    
    // Very simple parser for demonstration
    // A production implementation would use a proper expression parser
    
    // Handle single number
    if let Ok(num) = expr.parse::<f64>() {
        return Ok(num);
    }

    // Recursive descent: search for lowest-precedence operators first.
    // Using rfind means the rightmost operator becomes the split point,
    // giving correct left-to-right associativity.

    // Addition (lowest precedence — search first)
    if let Some(pos) = expr.rfind('+') {
        let left = evaluate(&expr[..pos])?;
        let right = evaluate(&expr[pos + 1..])?;
        return Ok(left + right);
    }

    // Subtraction (only if not at the start for negative numbers)
    if let Some(pos) = expr[1..].rfind('-') {
        let pos = pos + 1; // Adjust for the slice offset
        let left = evaluate(&expr[..pos])?;
        let right = evaluate(&expr[pos + 1..])?;
        return Ok(left - right);
    }

    // Multiplication (higher precedence)
    if let Some(pos) = expr.rfind('*') {
        let left = evaluate(&expr[..pos])?;
        let right = evaluate(&expr[pos + 1..])?;
        return Ok(left * right);
    }

    // Division (highest precedence — search last)
    if let Some(pos) = expr.rfind('/') {
        let left = evaluate(&expr[..pos])?;
        let right = evaluate(&expr[pos + 1..])?;
        if right == 0.0 {
            return Err("Division by zero".to_string());
        }
        return Ok(left / right);
    }

    Err(format!("Invalid expression: {}", expr))
}

/// Skill entry point
#[no_mangle]
pub extern "C" fn run() {
    log(2, "Calculator skill starting");

    // Get input
    let input = get_input();
    log(2, &format!("Received input: {}", input));

    // Parse input
    let query: CalculatorQuery = match serde_json::from_str(&input) {
        Ok(q) => q,
        Err(e) => {
            let error_msg = format!("Failed to parse input: {}", e);
            log(4, &error_msg);
            set_output(&format!(
                "{{\"error\": \"Invalid input format. Expected JSON with 'expression' field.\"}}"
            ));
            return;
        }
    };

    log(2, &format!("Evaluating: {}", query.expression));

    // Evaluate expression
    let result = match evaluate(&query.expression) {
        Ok(r) => r,
        Err(e) => {
            let error_msg = format!("Evaluation error: {}", e);
            log(4, &error_msg);
            set_output(&format!("{{\"error\": \"{}\"}}", error_msg));
            return;
        }
    };

    // Create response
    let response = CalculatorResponse {
        result,
        expression: query.expression.clone(),
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

    log(2, &format!("Result: {}", result));
    set_output(&response_json);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_single_number() {
        assert_eq!(evaluate("42").unwrap(), 42.0);
    }

    #[test]
    fn test_evaluate_addition() {
        assert_eq!(evaluate("2 + 3").unwrap(), 5.0);
    }

    #[test]
    fn test_evaluate_multiplication() {
        assert_eq!(evaluate("4 * 5").unwrap(), 20.0);
    }

    #[test]
    fn test_evaluate_division() {
        assert_eq!(evaluate("10 / 2").unwrap(), 5.0);
    }

    #[test]
    fn test_evaluate_subtraction() {
        assert_eq!(evaluate("10 - 3").unwrap(), 7.0);
    }

    #[test]
    fn test_evaluate_complex() {
        assert_eq!(evaluate("2 + 3 * 4").unwrap(), 14.0);
    }

    #[test]
    fn test_evaluate_division_by_zero() {
        assert!(evaluate("10 / 0").is_err());
    }

    #[test]
    fn test_calculator_query_parse() {
        let json = r#"{"expression": "2 + 2"}"#;
        let query: CalculatorQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.expression, "2 + 2");
    }

    #[test]
    fn test_calculator_response_serialize() {
        let response = CalculatorResponse {
            result: 42.0,
            expression: "40 + 2".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("42"));
    }
}

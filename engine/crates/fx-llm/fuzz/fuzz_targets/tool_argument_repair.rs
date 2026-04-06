#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let repaired = fx_llm::repair_tool_arguments_json(&input);

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&repaired) {
        if repaired.trim_start().starts_with('{') {
            assert!(
                value.is_object(),
                "repaired object-like JSON must remain an object"
            );
        }
    }

    let _ = fx_llm::parse_tool_arguments_object(&input);
});

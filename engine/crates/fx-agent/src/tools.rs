//! Tool definitions for Fawx phone actions.

use crate::claude::types::Tool;
use serde_json::json;

/// Get all Fawx action tools for Claude.
pub fn fawx_action_tools() -> Vec<Tool> {
    vec![
        tap_tool(),
        swipe_tool(),
        type_text_tool(),
        launch_app_tool(),
        go_home_tool(),
        go_back_tool(),
        read_screen_tool(),
    ]
}

/// Tap on a UI element or coordinates.
fn tap_tool() -> Tool {
    Tool::new(
        "tap",
        "Tap on a UI element by name or coordinates. Use element name when possible.",
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Element name or coordinates in format 'x,y'"
                }
            },
            "required": ["target"]
        }),
    )
}

/// Swipe in a direction.
fn swipe_tool() -> Tool {
    Tool::new(
        "swipe",
        "Swipe the screen in a direction (up, down, left, right).",
        json!({
            "type": "object",
            "properties": {
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"],
                    "description": "Direction to swipe"
                }
            },
            "required": ["direction"]
        }),
    )
}

/// Type text into the focused input field.
fn type_text_tool() -> Tool {
    Tool::new(
        "type_text",
        "Type text into the currently focused input field.",
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to type"
                }
            },
            "required": ["text"]
        }),
    )
}

/// Launch an application.
fn launch_app_tool() -> Tool {
    Tool::new(
        "launch_app",
        "Launch an application by name or package ID.",
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "App name or package ID (e.g., 'Chrome', 'com.android.chrome')"
                }
            },
            "required": ["name"]
        }),
    )
}

/// Navigate to home screen.
fn go_home_tool() -> Tool {
    Tool::new(
        "go_home",
        "Navigate to the home screen.",
        json!({
            "type": "object",
            "properties": {}
        }),
    )
}

/// Go back (back button).
fn go_back_tool() -> Tool {
    Tool::new(
        "go_back",
        "Press the back button to go back to the previous screen.",
        json!({
            "type": "object",
            "properties": {}
        }),
    )
}

/// Read current screen state.
fn read_screen_tool() -> Tool {
    Tool::new(
        "read_screen",
        "Read the current screen state including visible elements and text content.",
        json!({
            "type": "object",
            "properties": {}
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fawx_action_tools_count() {
        let tools = fawx_action_tools();
        assert_eq!(tools.len(), 7);
    }

    #[test]
    fn test_tap_tool() {
        let tool = tap_tool();
        assert_eq!(tool.name, "tap");
        assert!(!tool.description.is_empty());

        let schema = tool
            .input_schema
            .as_object()
            .expect("schema should be object");
        let props = schema["properties"]
            .as_object()
            .expect("should have properties");
        assert!(props.contains_key("target"));

        let required = schema["required"].as_array().expect("should have required");
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "target");
    }

    #[test]
    fn test_swipe_tool() {
        let tool = swipe_tool();
        assert_eq!(tool.name, "swipe");

        let schema = tool
            .input_schema
            .as_object()
            .expect("schema should be object");
        let props = schema["properties"]
            .as_object()
            .expect("should have properties");
        let direction = &props["direction"];

        let enum_values = direction["enum"].as_array().expect("should have enum");
        assert_eq!(enum_values.len(), 4);
        assert!(enum_values.contains(&json!("up")));
        assert!(enum_values.contains(&json!("down")));
        assert!(enum_values.contains(&json!("left")));
        assert!(enum_values.contains(&json!("right")));
    }

    #[test]
    fn test_type_text_tool() {
        let tool = type_text_tool();
        assert_eq!(tool.name, "type_text");

        let schema = tool
            .input_schema
            .as_object()
            .expect("schema should be object");
        let required = schema["required"].as_array().expect("should have required");
        assert_eq!(required[0], "text");
    }

    #[test]
    fn test_launch_app_tool() {
        let tool = launch_app_tool();
        assert_eq!(tool.name, "launch_app");

        let schema = tool
            .input_schema
            .as_object()
            .expect("schema should be object");
        let props = schema["properties"]
            .as_object()
            .expect("should have properties");
        assert!(props.contains_key("name"));
    }

    #[test]
    fn test_go_home_tool() {
        let tool = go_home_tool();
        assert_eq!(tool.name, "go_home");

        let schema = tool
            .input_schema
            .as_object()
            .expect("schema should be object");
        let props = schema["properties"]
            .as_object()
            .expect("should have properties");
        assert!(props.is_empty()); // No parameters required
    }

    #[test]
    fn test_go_back_tool() {
        let tool = go_back_tool();
        assert_eq!(tool.name, "go_back");

        let schema = tool
            .input_schema
            .as_object()
            .expect("schema should be object");
        let props = schema["properties"]
            .as_object()
            .expect("should have properties");
        assert!(props.is_empty()); // No parameters required
    }

    #[test]
    fn test_read_screen_tool() {
        let tool = read_screen_tool();
        assert_eq!(tool.name, "read_screen");

        let schema = tool
            .input_schema
            .as_object()
            .expect("schema should be object");
        let props = schema["properties"]
            .as_object()
            .expect("should have properties");
        assert!(props.is_empty()); // No parameters required
    }

    #[test]
    fn test_all_tools_have_valid_schemas() {
        let tools = fawx_action_tools();

        for tool in tools {
            // Each tool should have a name
            assert!(!tool.name.is_empty());

            // Each tool should have a description
            assert!(!tool.description.is_empty());

            // Each tool should have a valid JSON schema
            let schema = tool
                .input_schema
                .as_object()
                .expect("schema should be object");
            assert_eq!(schema["type"], "object");
            assert!(schema.contains_key("properties"));
        }
    }
}

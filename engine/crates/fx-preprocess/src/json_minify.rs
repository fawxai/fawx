/// Minify JSON blocks found in text.
///
/// Detects JSON objects/arrays (both inline and inside markdown code fences),
/// parses them with serde_json, and re-serializes compactly. Non-JSON text
/// and invalid JSON are left unchanged.
pub(crate) fn minify_json_blocks(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();

    while let Some(&(i, ch)) = chars.peek() {
        // Check for markdown code fence with JSON content
        if ch == '`' && text[i..].starts_with("```") {
            let fence_result = try_minify_code_fence(text, i);
            if let Some((replacement, end)) = fence_result {
                result.push_str(&replacement);
                advance_to(&mut chars, end);
                continue;
            }
        }

        // Check for standalone JSON block
        if ch == '{' || ch == '[' {
            let closing = if ch == '{' { '}' } else { ']' };
            if let Some(end) = find_balanced(text, i, ch, closing) {
                let candidate = &text[i..=end];
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(candidate) {
                    // serde_json::to_string only fails on non-finite floats,
                    // which from_str never produces, so this is safe.
                    if let Ok(compact) = serde_json::to_string(&val) {
                        result.push_str(&compact);
                        advance_to(&mut chars, end + 1);
                        continue;
                    }
                }
            }
        }

        result.push(ch);
        chars.next();
    }

    result
}

/// Try to minify JSON inside a markdown code fence starting at `start`.
/// Returns `(replacement, end_index)` if successful.
fn try_minify_code_fence(text: &str, start: usize) -> Option<(String, usize)> {
    let after_open = find_fence_end_of_opening(text, start)?;
    let tag = text[start + 3..after_open].trim();

    // Only process json-tagged or untagged fences that contain JSON
    if !tag.is_empty() && !tag.eq_ignore_ascii_case("json") {
        return None;
    }

    let content_start = after_open + 1; // skip newline after opening fence
    let close_fence = find_closing_fence(text, content_start)?;
    let content = text[content_start..close_fence].trim();

    let val: serde_json::Value = serde_json::from_str(content).ok()?;
    let compact = serde_json::to_string(&val).ok()?;

    let fence_end = close_fence + find_fence_line_len(text, close_fence);
    let replacement = format!("```json\n{compact}\n```");
    Some((replacement, fence_end))
}

/// Find the end of the opening fence line (position of the newline).
fn find_fence_end_of_opening(text: &str, start: usize) -> Option<usize> {
    text[start..].find('\n').map(|p| start + p)
}

/// Find the position of the closing ``` fence.
fn find_closing_fence(text: &str, from: usize) -> Option<usize> {
    let mut pos = from;
    for line in text[from..].lines() {
        if line.trim_start().starts_with("```") {
            return Some(pos);
        }
        pos += line.len() + 1; // +1 for newline
    }
    None
}

/// Length of the closing fence line (including trailing newline if present).
fn find_fence_line_len(text: &str, fence_start: usize) -> usize {
    let rest = &text[fence_start..];
    rest.find('\n').map_or(rest.len(), |p| p + 1)
}

/// Find the matching closing bracket for an opening bracket, respecting
/// nesting and string literals.
fn find_balanced(text: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;
    let bytes = text.as_bytes();

    for (offset, &b) in bytes[start..].iter().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if in_string {
            match b {
                b'\\' => escape_next = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            _ if b == open as u8 => depth += 1,
            _ if b == close as u8 => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Advance the peekable iterator to position `target`.
fn advance_to(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, target: usize) {
    while let Some(&(i, _)) = chars.peek() {
        if i >= target {
            break;
        }
        chars.next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_json_minified() {
        let input = "{\n  \"name\": \"test\",\n  \"value\": 42\n}";
        let result = minify_json_blocks(input);
        assert_eq!(result, r#"{"name":"test","value":42}"#);
    }

    #[test]
    fn json_in_code_fence() {
        let input = "```json\n{\n  \"a\": 1,\n  \"b\": 2\n}\n```";
        let result = minify_json_blocks(input);
        assert_eq!(result, "```json\n{\"a\":1,\"b\":2}\n```");
    }

    #[test]
    fn mixed_text_and_json() {
        let input = "Here is the result:\n{\"key\": \"value\",  \"n\": 1}";
        let result = minify_json_blocks(input);
        assert_eq!(result, "Here is the result:\n{\"key\":\"value\",\"n\":1}");
    }

    #[test]
    fn invalid_json_unchanged() {
        let input = "{not valid json at all}";
        let result = minify_json_blocks(input);
        assert_eq!(result, input);
    }

    #[test]
    fn nested_json() {
        let input = "{\n  \"outer\": {\n    \"inner\": [1, 2, 3]\n  }\n}";
        let result = minify_json_blocks(input);
        assert_eq!(result, r#"{"outer":{"inner":[1,2,3]}}"#);
    }

    #[test]
    fn json_array() {
        let input = "[\n  1,\n  2,\n  3\n]";
        let result = minify_json_blocks(input);
        assert_eq!(result, "[1,2,3]");
    }

    #[test]
    fn empty_input() {
        assert_eq!(minify_json_blocks(""), "");
    }

    #[test]
    fn case_insensitive_fence_tag() {
        let input = "```JSON\n{\n  \"a\": 1,\n  \"b\": 2\n}\n```";
        let result = minify_json_blocks(input);
        assert_eq!(result, "```json\n{\"a\":1,\"b\":2}\n```");
    }

    #[test]
    fn mixed_case_fence_tag() {
        let input = "```Json\n{\n  \"x\": 10\n}\n```";
        let result = minify_json_blocks(input);
        assert_eq!(result, "```json\n{\"x\":10}\n```");
    }
}

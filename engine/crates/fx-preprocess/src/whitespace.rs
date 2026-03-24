/// Collapse excessive whitespace in text.
///
/// - Multiple consecutive blank lines → single blank line
/// - Trailing whitespace on each line stripped
/// - Tabs → 2 spaces
/// - Content inside markdown code fences (```) is preserved as-is
/// - Single newlines between content lines are preserved
pub(crate) fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_code_fence = false;
    let mut prev_blank = false;

    for line in text.split('\n') {
        let trimmed = line.trim_start();

        // Toggle code fence state
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
        }

        if in_code_fence {
            // Inside code fence: preserve exactly as-is
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
            prev_blank = false;
            continue;
        }

        // Replace tabs with 2 spaces, then strip trailing whitespace
        let processed = line.replace('\t', "  ");
        let stripped = processed.trim_end();

        if stripped.is_empty() {
            // Blank line: collapse consecutive blanks
            if !prev_blank && !result.is_empty() {
                result.push('\n');
            }
            prev_blank = true;
        } else {
            if prev_blank {
                result.push('\n');
            }
            if !result.is_empty() && !prev_blank {
                result.push('\n');
            }
            result.push_str(stripped);
            prev_blank = false;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_blank_lines_collapsed() {
        let input = "line1\n\n\n\nline2";
        let result = collapse_whitespace(input);
        assert_eq!(result, "line1\n\nline2");
    }

    #[test]
    fn trailing_whitespace_stripped() {
        let input = "hello   \nworld  ";
        let result = collapse_whitespace(input);
        assert_eq!(result, "hello\nworld");
    }

    #[test]
    fn tabs_to_spaces() {
        let input = "\tindented\n\t\tdouble";
        let result = collapse_whitespace(input);
        assert_eq!(result, "  indented\n    double");
    }

    #[test]
    fn code_fence_preserved() {
        let input = "before\n```\n  lots   of   spaces  \n\n\n```\nafter";
        let result = collapse_whitespace(input);
        assert_eq!(
            result,
            "before\n```\n  lots   of   spaces  \n\n\n```\nafter"
        );
    }

    #[test]
    fn single_newlines_preserved() {
        let input = "paragraph one\nstill paragraph one\n\nparagraph two";
        let result = collapse_whitespace(input);
        assert_eq!(
            result,
            "paragraph one\nstill paragraph one\n\nparagraph two"
        );
    }
}

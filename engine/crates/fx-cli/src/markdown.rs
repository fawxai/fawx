// Incremental markdown renderer for streaming TUI output (#1097).
//
// Accumulates raw delta text and emits ANSI-formatted output for
// complete markdown elements while deferring incomplete ones.

use crossterm::style::Stylize;

/// Incremental markdown renderer that maintains parse state across deltas.
///
/// Call [`push`] with each streaming delta; it returns ANSI-formatted text
/// ready for printing. Call [`flush`] at the end to emit any trailing content.
pub(crate) struct MarkdownRenderer {
    /// Unprocessed text waiting for a closing marker or newline.
    pending: String,
    /// Whether we are inside a fenced code block.
    in_code_block: bool,
}

impl MarkdownRenderer {
    pub(crate) fn new() -> Self {
        Self {
            pending: String::new(),
            in_code_block: false,
        }
    }

    /// Append a streaming delta and return ANSI-formatted output for any
    /// complete elements discovered so far.
    pub(crate) fn push(&mut self, text: &str) -> String {
        self.pending.push_str(text);
        self.drain_complete_lines()
    }

    /// Flush remaining pending text (call on finalize).
    pub(crate) fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        let tail = std::mem::take(&mut self.pending);
        if self.in_code_block {
            format_code_block_line(&tail)
        } else {
            format_inline(&tail)
        }
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Process all complete lines (terminated by `\n`) in `pending`,
    /// leaving any trailing partial line for the next delta.
    fn drain_complete_lines(&mut self) -> String {
        let mut output = String::new();

        loop {
            let Some(nl_pos) = self.pending.find('\n') else {
                break;
            };

            // Split off the complete line (including the newline).
            let line: String = self.pending.drain(..=nl_pos).collect();
            let trimmed = line.trim_end_matches('\n');

            output.push_str(&self.render_line(trimmed));
            output.push('\n');
        }

        output
    }

    /// Render a single complete line with markdown formatting.
    fn render_line(&mut self, line: &str) -> String {
        // Code-fence toggle: only exact ``` or ``` followed by whitespace/lang tag
        if is_code_fence(line) {
            self.in_code_block = !self.in_code_block;
            let separator = "─".repeat(40);
            return format!("{}", separator.dim());
        }

        if self.in_code_block {
            return format_code_block_line(line);
        }

        // Headers
        if let Some(rest) = line.strip_prefix("### ") {
            return format_header(rest, 3);
        }
        if let Some(rest) = line.strip_prefix("## ") {
            return format_header(rest, 2);
        }
        if let Some(rest) = line.strip_prefix("# ") {
            return format_header(rest, 1);
        }

        // List items — preserve bullet, format content
        if let Some(content) = strip_list_prefix(line) {
            let prefix = &line[..line.len() - content.len()];
            return format!("{}{}", prefix, format_inline(content));
        }

        format_inline(line)
    }
}

fn term_indicates_truecolor(term: &str) -> bool {
    term.ends_with("-direct") || term == "xterm-direct" || term.contains("truecolor")
}

fn supports_truecolor() -> bool {
    if let Ok(value) = std::env::var("COLORTERM") {
        if value == "truecolor" || value == "24bit" {
            return true;
        }
    }

    if let Ok(term) = std::env::var("TERM") {
        return term_indicates_truecolor(&term);
    }

    false
}

fn theme_color(r: u8, g: u8, b: u8, fallback_256: u8) -> crossterm::style::Color {
    if supports_truecolor() {
        crossterm::style::Color::Rgb { r, g, b }
    } else {
        crossterm::style::Color::AnsiValue(fallback_256)
    }
}

/// Format a header line with bold + color, differentiated by level.
fn format_header(text: &str, level: u8) -> String {
    match level {
        1 => {
            let bright = theme_color(100, 220, 255, 81);
            format!("{}", text.bold().underlined().with(bright))
        }
        2 => {
            let medium = theme_color(100, 200, 255, 75);
            format!("{}", text.bold().with(medium))
        }
        _ => {
            let dim = theme_color(130, 190, 230, 110);
            format!("{}", text.bold().with(dim))
        }
    }
}

/// Format a code-block line: cyan/dim, indented.
fn format_code_block_line(line: &str) -> String {
    let green = theme_color(120, 220, 120, 114);
    format!("  {}", line.with(green))
}

/// Strip a list-item prefix (`- `, `* `, `1. `, etc.) and return the
/// remaining content, or `None` if the line isn't a list item.
fn strip_list_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return Some(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("+ ") {
        return Some(rest);
    }
    // Ordered list: digits followed by `. `
    if let Some(dot_pos) = trimmed.find(". ") {
        let prefix = &trimmed[..dot_pos];
        if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
            return Some(&trimmed[dot_pos + 2..]);
        }
    }
    None
}

/// Apply inline formatting (bold, italic, inline code) to a text fragment.
///
/// Uses a simple single-pass scanner. If anything goes wrong, the original
/// text is returned unmodified (graceful fallback).
fn format_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Inline code: `...`
        if chars[i] == '`' {
            if let Some(end) = find_closing(&chars, i + 1, '`') {
                let code: String = chars[i + 1..end].iter().collect();
                let cyan = theme_color(180, 220, 255, 117);
                out.push_str(&format!("{}", code.with(cyan)));
                i = end + 1;
                continue;
            }
        }

        // Bold: **...**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_double_closing(&chars, i + 2) {
                let bold_text: String = chars[i + 2..end].iter().collect();
                out.push_str(&format!("{}", bold_text.bold()));
                i = end + 2;
                continue;
            }
        }

        // Italic: *...*  (single asterisk, not followed by another)
        if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
            if let Some(end) = find_closing_single_star(&chars, i + 1) {
                let italic_text: String = chars[i + 1..end].iter().collect();
                out.push_str(&format!("{}", italic_text.italic()));
                i = end + 1;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Check if a line is a valid code fence: exactly ``` optionally followed by
/// a language tag (alphanumeric + hyphens) or whitespace only.
fn is_code_fence(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with("```") {
        return false;
    }
    let after = &trimmed[3..];
    // Opening fence: ``` optionally followed by a language identifier
    // Closing fence: exactly ```
    after.is_empty()
        || after
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Find closing backtick starting from `start`.
fn find_closing(chars: &[char], start: usize, marker: char) -> Option<usize> {
    (start..chars.len()).find(|&j| chars[j] == marker)
}

/// Find closing `**` starting from `start`.
fn find_double_closing(chars: &[char], start: usize) -> Option<usize> {
    let len = chars.len();
    let mut j = start;
    while j + 1 < len {
        if chars[j] == '*' && chars[j + 1] == '*' {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Find closing single `*` that is NOT part of `**`.
fn find_closing_single_star(chars: &[char], start: usize) -> Option<usize> {
    let len = chars.len();
    for j in start..len {
        if chars[j] == '*' {
            // Make sure it's not part of **
            let next_is_star = j + 1 < len && chars[j + 1] == '*';
            let prev_is_star = j > 0 && chars[j - 1] == '*';
            if !next_is_star && !prev_is_star {
                return Some(j);
            }
        }
    }
    None
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_header() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("# Hello\n");
        assert!(out.contains("Hello"), "should contain header text");
        assert!(out.contains("\x1b["), "should contain ANSI codes");
    }

    #[test]
    fn renders_bold() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("some **bold** text\n");
        assert!(out.contains("\x1b[1m"), "should contain bold ANSI");
        assert!(out.contains("bold"), "should contain bold text");
        assert!(!out.contains("**"), "should not contain raw markers");
    }

    #[test]
    fn renders_italic() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("some *italic* text\n");
        assert!(out.contains("\x1b[3m"), "should contain italic ANSI");
        assert!(!out.contains("*italic*"), "raw markers removed");
    }

    #[test]
    fn renders_inline_code() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("use `foo()` here\n");
        assert!(out.contains("foo()"), "code text present");
        assert!(!out.contains('`'), "backticks removed");
    }

    #[test]
    fn renders_code_block() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("```rust\nlet x = 1;\n```\n");
        assert!(out.contains("let x = 1;"), "code line present");
        assert!(out.contains("─"), "fence rendered as separator");
    }

    #[test]
    fn defers_incomplete_line() {
        let mut r = MarkdownRenderer::new();
        // No newline — should be buffered
        let out = r.push("# Hello");
        assert!(out.is_empty(), "incomplete line deferred");
        // Complete it
        let out = r.push(" World\n");
        assert!(out.contains("Hello World"), "complete line rendered");
    }

    #[test]
    fn flush_emits_trailing_content() {
        let mut r = MarkdownRenderer::new();
        r.push("trailing text");
        let out = r.flush();
        assert!(out.contains("trailing text"));
    }

    #[test]
    fn list_items_preserved() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("- item one\n1. item two\n");
        assert!(out.contains("- "), "bullet preserved");
        assert!(out.contains("1. "), "ordered prefix preserved");
    }

    #[test]
    fn graceful_fallback_unclosed_bold() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("some **unclosed bold\n");
        // Should still contain the text, just without formatting
        assert!(
            out.contains("**unclosed bold"),
            "unclosed marker passed through"
        );
    }

    #[test]
    fn multi_delta_bold() {
        let mut r = MarkdownRenderer::new();
        // Bold split across deltas, line not yet complete
        let out1 = r.push("hello **wor");
        assert!(out1.is_empty(), "no complete line yet");
        let out2 = r.push("ld** done\n");
        assert!(
            out2.contains("\x1b[1m"),
            "bold rendered after line complete"
        );
        assert!(out2.contains("world"), "full bold text present");
    }

    #[test]
    fn code_block_state_persists_across_deltas() {
        let mut r = MarkdownRenderer::new();
        let _ = r.push("```\n");
        assert!(r.in_code_block);
        let out = r.push("inside code\n");
        assert!(out.contains("inside code"));
        let _ = r.push("```\n");
        assert!(!r.in_code_block);
    }

    #[test]
    fn empty_push() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("");
        assert!(out.is_empty());
    }

    #[test]
    fn empty_flush() {
        let mut r = MarkdownRenderer::new();
        let out = r.flush();
        assert!(out.is_empty());
    }

    #[test]
    fn multi_format_bold_and_inline_code() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("use **bold** and `code` together\n");
        assert!(out.contains("bold"), "bold text present");
        assert!(out.contains("code"), "code text present");
        assert!(!out.contains("**"), "bold markers removed");
        assert!(!out.contains('`'), "backtick markers removed");
    }

    #[test]
    fn flush_inside_code_block() {
        let mut r = MarkdownRenderer::new();
        let _ = r.push("```\n");
        assert!(r.in_code_block);
        r.push("partial code");
        let out = r.flush();
        assert!(out.contains("partial code"), "flushed code block content");
    }

    #[test]
    fn nested_formatting_bold_with_backtick_inside() {
        let mut r = MarkdownRenderer::new();
        // Bold wrapping text that contains a backtick (unclosed) — should
        // still render bold and pass through the backtick.
        let out = r.push("**contains ` char** rest\n");
        assert!(out.contains("contains"), "bold text rendered");
    }

    #[test]
    fn plus_list_prefix() {
        let mut r = MarkdownRenderer::new();
        let out = r.push("+ item plus\n");
        assert!(out.contains("+ "), "plus bullet preserved");
        assert!(out.contains("item plus"), "content present");
    }

    #[test]
    fn code_fence_with_trailing_content_rejected() {
        let mut r = MarkdownRenderer::new();
        // ``` followed by non-lang content should NOT toggle code block
        let _out = r.push("``` this is not a fence {}\n");
        assert!(
            !r.in_code_block,
            "trailing non-identifier content should not open code block"
        );
    }

    #[test]
    fn code_fence_with_language_tag() {
        let mut r = MarkdownRenderer::new();
        let _ = r.push("```rust\n");
        assert!(r.in_code_block, "language-tagged fence opens block");
        let _ = r.push("```\n");
        assert!(!r.in_code_block, "plain fence closes block");
    }

    #[test]
    fn header_levels_differ() {
        let mut r = MarkdownRenderer::new();
        let h1 = r.push("# Title\n");
        let h2 = r.push("## Subtitle\n");
        let h3 = r.push("### Section\n");
        // All should contain their text
        assert!(h1.contains("Title"));
        assert!(h2.contains("Subtitle"));
        assert!(h3.contains("Section"));
        // h1 should have underline ANSI (distinguishing from h2/h3)
        assert!(
            h1.contains("\x1b[4m") || h1.contains("4m"),
            "h1 has underline"
        );
    }
}

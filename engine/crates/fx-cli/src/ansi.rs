//! Lightweight ANSI escape → ratatui span converter.
//!
//! Supports:
//! - `\x1b[38;2;R;G;Bm` — 24-bit foreground colour
//! - `\x1b[0m`           — reset all attributes
//!
//! Other CSI sequences are silently stripped.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Current parsing state for the ANSI FSM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Normal text output.
    Text,
    /// Saw `\x1b`, expecting `[`.
    Escape,
    /// Inside a CSI parameter sequence (digits and `;`).
    Csi,
}

/// Parse a single string containing ANSI escape sequences and return
/// a ratatui [`Line`] with the corresponding styled [`Span`]s.
pub fn ansi_to_line(input: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut fg: Option<Color> = None;
    let mut state = State::Text;
    let mut csi_buf = String::new();

    for ch in input.chars() {
        match state {
            State::Text => {
                if ch == '\x1b' {
                    state = State::Escape;
                } else {
                    buf.push(ch);
                }
            }
            State::Escape => {
                if ch == '[' {
                    state = State::Csi;
                    csi_buf.clear();
                } else {
                    // Not a CSI sequence — emit the ESC as-is and go back.
                    buf.push('\x1b');
                    buf.push(ch);
                    state = State::Text;
                }
            }
            State::Csi => {
                if ('@'..='~').contains(&ch) {
                    // Final byte — flush the preceding text, then apply.
                    flush_span(&mut spans, &mut buf, fg);
                    if ch == 'm' {
                        fg = apply_sgr(&csi_buf, fg);
                    }
                    // Other CSI finals (e.g. 'H', 'J') are silently dropped.
                    state = State::Text;
                } else {
                    csi_buf.push(ch);
                }
            }
        }
    }

    flush_span(&mut spans, &mut buf, fg);

    if spans.is_empty() {
        Line::raw(String::new())
    } else {
        Line::from(spans)
    }
}

/// Flush accumulated text into a styled span.
fn flush_span(spans: &mut Vec<Span<'static>>, buf: &mut String, fg: Option<Color>) {
    if buf.is_empty() {
        return;
    }
    let style = match fg {
        Some(color) => Style::default().fg(color),
        None => Style::default(),
    };
    spans.push(Span::styled(std::mem::take(buf), style));
}

/// Interpret an SGR (Select Graphic Rendition) parameter string.
///
/// Returns the new foreground colour (or `None` for reset).
fn apply_sgr(params: &str, current: Option<Color>) -> Option<Color> {
    if params.is_empty() || params == "0" {
        return None; // reset
    }
    parse_sgr_rgb(params).or(current)
}

/// Try to parse `38;2;R;G;B` from an SGR parameter string.
fn parse_sgr_rgb(params: &str) -> Option<Color> {
    let parts: Vec<&str> = params.split(';').collect();
    if parts.len() == 5 && parts[0] == "38" && parts[1] == "2" {
        let r = parts[2].parse::<u8>().ok()?;
        let g = parts[3].parse::<u8>().ok()?;
        let b = parts[4].parse::<u8>().ok()?;
        return Some(Color::Rgb(r, g, b));
    }
    None
}

/// Returns `true` when the string contains at least one ANSI CSI escape.
pub fn contains_ansi(input: &str) -> bool {
    input.contains("\x1b[")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passthrough() {
        let line = ansi_to_line("hello world");
        let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "hello world");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].style, Style::default());
    }

    #[test]
    fn single_rgb_foreground() {
        let input = "\x1b[38;2;255;128;0mhello\x1b[0m";
        let line = ansi_to_line(input);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content.as_ref(), "hello");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Rgb(255, 128, 0))
        );
    }

    #[test]
    fn reset_returns_to_default() {
        let input = "\x1b[38;2;10;20;30mfoo\x1b[0mbar";
        let line = ansi_to_line(input);
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content.as_ref(), "foo");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Rgb(10, 20, 30))
        );
        assert_eq!(line.spans[1].content.as_ref(), "bar");
        assert_eq!(line.spans[1].style, Style::default());
    }

    #[test]
    fn empty_input() {
        let line = ansi_to_line("");
        assert!(line.spans.is_empty() || line.spans[0].content.is_empty());
    }

    #[test]
    fn contains_ansi_detects_escapes() {
        assert!(contains_ansi("\x1b[38;2;1;2;3mhello\x1b[0m"));
        assert!(!contains_ansi("plain text"));
    }

    #[test]
    fn hero_art_is_non_empty() {
        let art = include_str!("../../../../docs/fawx-hero-ansi.txt");
        assert!(!art.is_empty());
        let lines: Vec<&str> = art.lines().collect();
        assert!(lines.len() >= 30, "expected ≥30 lines, got {}", lines.len());
    }

    #[test]
    fn hero_art_lines_parse_without_panic() {
        let art = include_str!("../../../../docs/fawx-hero-ansi.txt");
        for line in art.lines() {
            let _ = ansi_to_line(line);
        }
    }

    #[test]
    fn multiple_colours_on_one_line() {
        let input = "\x1b[38;2;255;0;0mred\x1b[38;2;0;255;0mgreen\x1b[0m";
        let line = ansi_to_line(input);
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content.as_ref(), "red");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Rgb(255, 0, 0))
        );
        assert_eq!(line.spans[1].content.as_ref(), "green");
        assert_eq!(
            line.spans[1].style,
            Style::default().fg(Color::Rgb(0, 255, 0))
        );
    }
}

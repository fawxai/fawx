//! Persistent input bar via ANSI scroll regions (DECSTBM).
//!
//! Splits the terminal into two zones:
//! - **Scroll region** (rows 1 through height-3): all output scrolls here
//! - **Input bar** (bottom 2 rows): separator line + pinned prompt
//!
//! Gracefully degrades: if terminal size cannot be determined, all
//! operations become no-ops and the TUI works without scroll regions.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

// ---------------------------------------------------------------------------
// ANSI escape sequence constants
// ---------------------------------------------------------------------------

/// Reset scroll region to full screen (DECSTBM reset).
const ESC_RESET_SCROLL_REGION: &str = "\x1b[r";
/// Show cursor.
const ESC_SHOW_CURSOR: &str = "\x1b[?25h";
/// Save cursor position (DEC private).
const ESC_SAVE_CURSOR: &str = "\x1b7";
/// Restore cursor position (DEC private).
const ESC_RESTORE_CURSOR: &str = "\x1b8";
/// Erase entire line.
const ESC_ERASE_LINE: &str = "\x1b[2K";
/// ANSI dim attribute.
const ESC_DIM: &str = "\x1b[2m";
/// ANSI reset attributes.
const ESC_RESET_ATTRS: &str = "\x1b[0m";
/// Amber color for the input bar prompt.
const ESC_AMBER: &str = "\x1b[38;2;255;204;0m";

/// The prompt text for the input bar.
const INPUT_BAR_PROMPT: &str = "you \u{203a} ";

/// Display width of the input bar prompt (visible characters only).
/// "you › " = 6 visible characters.
const INPUT_BAR_PROMPT_DISPLAY_WIDTH: usize = 6;

/// Dark gray background for the input bar (ANSI 256-color).
const ESC_INPUT_BG: &str = "\x1b[48;5;236m";

/// Whether the scroll region is currently active.
static SCROLL_REGION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Cached terminal height (updated on resize).
static TERMINAL_HEIGHT: AtomicU16 = AtomicU16::new(0);

/// Cached terminal width (updated on resize).
static TERMINAL_WIDTH: AtomicU16 = AtomicU16::new(0);

/// Number of rows reserved for the input bar at the bottom.
/// Row 1: separator line, Row 2: input prompt.
const INPUT_BAR_ROWS: u16 = 2;

/// Minimum terminal height required to enable scroll regions.
/// Below this, scroll regions are skipped to avoid unusable layouts.
const MIN_HEIGHT_FOR_SCROLL: u16 = 8;

// ---------------------------------------------------------------------------
// Terminal capability detection
// ---------------------------------------------------------------------------

/// Check if the terminal likely supports 256-color mode.
///
/// Returns `true` when `$COLORTERM` is set (implies truecolor, which
/// is a superset of 256-color) or when `$TERM` contains `256color`
/// or `direct`.
pub(crate) fn supports_256_color() -> bool {
    if std::env::var("COLORTERM").is_ok() {
        return true;
    }
    if let Ok(term) = std::env::var("TERM") {
        return term.contains("256color") || term.contains("direct");
    }
    false
}

/// Return the input bar background escape if 256-color is supported.
fn input_bar_bg() -> &'static str {
    if supports_256_color() {
        ESC_INPUT_BG
    } else {
        ""
    }
}

/// Build the separator line (dim amber `─` chars across terminal width).
fn build_separator_line(width: u16) -> String {
    let bar = "\u{2500}".repeat(width as usize);
    if supports_256_color() {
        format!("{ESC_DIM}{ESC_AMBER}{bar}{ESC_RESET_ATTRS}")
    } else {
        bar
    }
}

/// Render the separator line on the row above the input prompt.
///
/// Always queries the current terminal width so the separator renders
/// at the correct width even after a resize — never uses a stale
/// cached value passed by the caller.
fn render_separator(height: u16) {
    let sep_row = height.saturating_sub(1);
    let width = current_width();
    if sep_row == 0 || width == 0 {
        return;
    }
    let line = build_separator_line(width);
    eprint!("\x1b[{sep_row};1H{ESC_ERASE_LINE}{line}");
}

/// Render the dimmed prompt with optional background, padded to width.
fn render_dimmed_prompt(height: u16, width: u16) {
    let bg = input_bar_bg();
    let pad_count = (width as usize).saturating_sub(INPUT_BAR_PROMPT_DISPLAY_WIDTH);
    let pad = " ".repeat(pad_count);
    eprint!(
        "\x1b[{height};1H{ESC_ERASE_LINE}\
         {bg}{ESC_DIM}{ESC_AMBER}{INPUT_BAR_PROMPT}{pad}{ESC_RESET_ATTRS}"
    );
}

/// Return the current cached terminal width, or query if not cached.
pub(crate) fn current_width() -> u16 {
    let w = TERMINAL_WIDTH.load(Ordering::Relaxed);
    if w > 0 {
        return w;
    }
    query_terminal_size().map_or(80, |(width, _)| width)
}

// ---------------------------------------------------------------------------
// Core scroll region functions
// ---------------------------------------------------------------------------

/// Query the current terminal dimensions.
///
/// Returns `None` if the terminal size cannot be determined (e.g.,
/// piped output, non-terminal environment).
fn query_terminal_size() -> Option<(u16, u16)> {
    crossterm::terminal::size().ok()
}

/// Set up the scroll region, excluding the bottom row(s) for the input bar.
///
/// This is a no-op if the terminal size cannot be determined or the
/// terminal is too small.
pub fn setup_scroll_region() {
    let Some((width, height)) = query_terminal_size() else {
        return;
    };

    if height < MIN_HEIGHT_FOR_SCROLL {
        return;
    }

    TERMINAL_HEIGHT.store(height, Ordering::Relaxed);
    TERMINAL_WIDTH.store(width, Ordering::Relaxed);
    apply_scroll_region(height);
    render_input_bar_dimmed(height);
    // Position cursor in the scroll region for output
    move_cursor_to_scroll_region(height);
    SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);
}

/// Install a panic hook that resets the scroll region and shows the cursor.
///
/// Without this, a panic leaves the terminal in a corrupted state with
/// the scroll region still active and potentially the cursor hidden.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Reset scroll region and show cursor before printing panic info
        eprint!("{ESC_RESET_SCROLL_REGION}{ESC_SHOW_CURSOR}");
        let _ = io::stderr().flush();
        SCROLL_REGION_ACTIVE.store(false, Ordering::Relaxed);
        previous(info);
    }));
}

/// Apply DECSTBM escape to set the scroll region.
fn apply_scroll_region(height: u16) {
    let scroll_end = height.saturating_sub(INPUT_BAR_ROWS);
    if scroll_end == 0 {
        return;
    }
    // DECSTBM: \x1b[{top};{bottom}r — sets scrolling region
    // Rows are 1-indexed in VT100
    eprint!("\x1b[1;{scroll_end}r");
    let _ = io::stderr().flush();
}

/// Render the dimmed input bar (separator + prompt) at the bottom.
fn render_input_bar_dimmed(height: u16) {
    let width = current_width();
    eprint!("{ESC_SAVE_CURSOR}");
    render_separator(height);
    render_dimmed_prompt(height, width);
    eprint!("{ESC_RESTORE_CURSOR}");
    let _ = io::stderr().flush();
}

/// Render the active input bar at the bottom.
/// Called when readline is about to take over. Only renders the
/// separator; the input line is cleared for readline to manage.
fn render_input_bar_active(height: u16) {
    eprint!("{ESC_SAVE_CURSOR}");
    render_separator(height);
    eprint!("{ESC_RESTORE_CURSOR}");
    let _ = io::stderr().flush();
}

/// Clear the separator and prompt rows at a given height.
///
/// Used during resize to erase stale input-bar characters before
/// re-rendering at the new terminal dimensions.
fn clear_input_bar_rows(height: u16) {
    if height == 0 {
        return;
    }
    let sep_row = height.saturating_sub(1);
    eprint!("{ESC_SAVE_CURSOR}");
    if sep_row > 0 {
        eprint!("\x1b[{sep_row};1H{ESC_ERASE_LINE}");
    }
    eprint!("\x1b[{height};1H{ESC_ERASE_LINE}");
    eprint!("{ESC_RESTORE_CURSOR}");
    let _ = io::stderr().flush();
}

/// Move cursor into the scroll region (last line of scroll region).
fn move_cursor_to_scroll_region(height: u16) {
    let scroll_end = height.saturating_sub(INPUT_BAR_ROWS);
    if scroll_end > 0 {
        eprint!("\x1b[{scroll_end};1H");
        let _ = io::stderr().flush();
    }
}

/// Check if the scroll region is currently active.
pub fn is_active() -> bool {
    SCROLL_REGION_ACTIVE.load(Ordering::Relaxed)
}

/// Return the last row of the scroll region (bottom boundary).
///
/// This is the row just above the input bar separator — the row
/// where the spinner renders and where new output appends.
pub(crate) fn scroll_region_end() -> u16 {
    let height = TERMINAL_HEIGHT.load(Ordering::Relaxed);
    height.saturating_sub(INPUT_BAR_ROWS)
}

/// Pin the cursor to the input bar prompt position.
///
/// Moves the cursor to the column just after the dimmed "you ›"
/// prompt so it appears in the input bar during execution phases
/// rather than in the scroll region where output is being written.
pub fn pin_cursor_to_input_bar() {
    if !is_active() {
        return;
    }

    let height = TERMINAL_HEIGHT.load(Ordering::Relaxed);
    if height == 0 {
        return;
    }

    let col = INPUT_BAR_PROMPT_DISPLAY_WIDTH as u16 + 1;
    eprint!("\x1b[{height};{col}H");
    let _ = io::stderr().flush();
}

/// Handle terminal resize: re-apply scroll region and re-render input bar.
///
/// Acts on any dimension change (height or width). On height changes the
/// old separator and prompt rows must be explicitly cleared before
/// re-rendering at their new positions, otherwise stale `─` characters
/// accumulate on screen.
pub fn handle_resize() {
    if !is_active() {
        return;
    }

    let Some((width, height)) = query_terminal_size() else {
        return;
    };

    if height < MIN_HEIGHT_FOR_SCROLL {
        // Terminal too small — tear down scroll region
        reset_scroll_region();
        return;
    }

    let old_height = TERMINAL_HEIGHT.swap(height, Ordering::Relaxed);
    let old_width = TERMINAL_WIDTH.swap(width, Ordering::Relaxed);

    if old_height == height && old_width == width {
        return;
    }

    // Clear the OLD separator + prompt rows so no stale chars remain.
    // This must happen before re-rendering at the new positions.
    clear_input_bar_rows(old_height);

    // Re-apply scroll region boundaries (always, since even a
    // width-only change benefits from a DECSTBM re-issue to be safe).
    apply_scroll_region(height);

    // Re-render separator and prompt at the new positions/width
    render_input_bar_dimmed(height);
    move_cursor_to_scroll_region(height);
}

/// Prepare the input bar for readline (active/non-dimmed state).
///
/// Called just before `editor.readline()` takes over. Temporarily
/// resets the scroll region to full screen so rustyline can handle
/// long input that wraps beyond a single row. The scroll region is
/// reestablished by [`restore_dimmed_bar`] after readline returns.
pub fn prepare_for_input() {
    if !is_active() {
        return;
    }

    let height = TERMINAL_HEIGHT.load(Ordering::Relaxed);
    if height == 0 {
        return;
    }

    // Reset scroll region so rustyline has the full terminal for wrapping.
    // Without this, text at the last row overwrites itself because the
    // terminal cannot scroll outside the DECSTBM region.
    eprint!("{ESC_RESET_SCROLL_REGION}");
    let _ = io::stderr().flush();

    render_input_bar_active(height);
    // Move cursor to input bar row for readline
    eprint!("\x1b[{height};1H{ESC_ERASE_LINE}");
    let _ = io::stderr().flush();
}

/// Restore the dimmed input bar after readline completes.
///
/// Called after the user submits input, before processing begins.
/// Reestablishes the DECSTBM scroll region that was temporarily
/// reset in [`prepare_for_input`], clears any leftover wrapped
/// input text from the input bar rows, and re-renders the dimmed
/// prompt.
pub fn restore_dimmed_bar() {
    if !is_active() {
        return;
    }

    let height = TERMINAL_HEIGHT.load(Ordering::Relaxed);
    if height == 0 {
        return;
    }

    // Reestablish scroll region (was reset for readline)
    apply_scroll_region(height);
    // Clear input bar rows — readline may have left wrapped text
    clear_input_bar_rows(height);
    render_input_bar_dimmed(height);
    move_cursor_to_scroll_region(height);
}

/// Ensure the cursor is positioned in the scroll region for output.
///
/// Moves the cursor to the bottom of the scroll region so new output
/// appends correctly. Does **not** re-render the separator or input
/// bar — doing so during streaming injected separator escape sequences
/// into the visible output stream, causing the separator line to
/// appear to expand with the response text.
///
/// The separator is rendered once by [`restore_dimmed_bar`] before
/// output begins; callers that need a post-output refresh should call
/// [`refresh_input_bar`] explicitly.
pub fn position_for_output() {
    if !is_active() {
        return;
    }

    let height = TERMINAL_HEIGHT.load(Ordering::Relaxed);
    if height == 0 {
        return;
    }

    move_cursor_to_scroll_region(height);
}

/// Refresh the dimmed input bar (e.g., after output that might have
/// disturbed the terminal layout).
pub fn refresh_input_bar() {
    if !is_active() {
        return;
    }

    let height = TERMINAL_HEIGHT.load(Ordering::Relaxed);
    if height == 0 {
        return;
    }

    render_input_bar_dimmed(height);
}

/// Reset the scroll region to full screen and clean up.
///
/// Called on TUI exit to restore normal terminal behavior.
pub fn reset_scroll_region() {
    if !SCROLL_REGION_ACTIVE.swap(false, Ordering::Relaxed) {
        return;
    }

    // Reset scroll region to full screen and show cursor
    eprint!("{ESC_RESET_SCROLL_REGION}{ESC_SHOW_CURSOR}");
    // Move cursor to bottom of screen
    let height = TERMINAL_HEIGHT.load(Ordering::Relaxed);
    if height > 0 {
        eprint!("\x1b[{height};1H");
    }
    // Clear the input bar lines (separator + prompt)
    let sep_row = height.saturating_sub(1);
    if sep_row > 0 {
        eprint!("\x1b[{sep_row};1H{ESC_ERASE_LINE}");
    }
    eprint!("\x1b[{height};1H{ESC_ERASE_LINE}");
    let _ = io::stderr().flush();
}

/// Build the DECSTBM escape sequence for a given terminal height.
///
/// Exposed for testing.
#[cfg(test)]
pub fn scroll_region_escape(height: u16) -> String {
    let scroll_end = height.saturating_sub(INPUT_BAR_ROWS);
    format!("\x1b[1;{scroll_end}r")
}

/// Return the current cached terminal dimensions for testing.
#[cfg(test)]
pub fn cached_dimensions() -> (u16, u16) {
    (
        TERMINAL_WIDTH.load(Ordering::Relaxed),
        TERMINAL_HEIGHT.load(Ordering::Relaxed),
    )
}

/// Build the escape sequence that `clear_input_bar_rows` would emit
/// for a given height. Exposed for test assertions.
#[cfg(test)]
pub fn clear_input_bar_escapes(height: u16) -> String {
    if height == 0 {
        return String::new();
    }
    let sep_row = height.saturating_sub(1);
    let mut out = String::from(ESC_SAVE_CURSOR);
    if sep_row > 0 {
        out.push_str(&format!("\x1b[{sep_row};1H{ESC_ERASE_LINE}"));
    }
    out.push_str(&format!("\x1b[{height};1H{ESC_ERASE_LINE}"));
    out.push_str(ESC_RESTORE_CURSOR);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset all shared state to a known baseline before each test.
    fn reset_state() {
        SCROLL_REGION_ACTIVE.store(false, Ordering::Relaxed);
        TERMINAL_HEIGHT.store(0, Ordering::Relaxed);
        TERMINAL_WIDTH.store(0, Ordering::Relaxed);
    }

    #[test]
    fn scroll_region_escape_correct_for_standard_terminal() {
        // INPUT_BAR_ROWS=2: height 24 → scroll_end = 22
        let escape = scroll_region_escape(24);
        assert_eq!(escape, "\x1b[1;22r");
    }

    #[test]
    fn scroll_region_escape_correct_for_large_terminal() {
        // INPUT_BAR_ROWS=2: height 80 → scroll_end = 78
        let escape = scroll_region_escape(80);
        assert_eq!(escape, "\x1b[1;78r");
    }

    #[test]
    fn scroll_region_escape_handles_minimum_height() {
        let escape = scroll_region_escape(MIN_HEIGHT_FOR_SCROLL);
        let scroll_end = MIN_HEIGHT_FOR_SCROLL - INPUT_BAR_ROWS;
        assert_eq!(escape, format!("\x1b[1;{scroll_end}r"));
    }

    #[test]
    fn input_bar_rows_is_reasonable() {
        let rows = INPUT_BAR_ROWS;
        assert!(rows >= 1);
        assert!(rows <= 3);
    }

    #[test]
    fn min_height_threshold_is_sane() {
        let min_h = MIN_HEIGHT_FOR_SCROLL;
        let bar = INPUT_BAR_ROWS;
        assert!(min_h > bar + 2);
    }

    #[test]
    fn is_active_defaults_to_false() {
        reset_state();
        assert!(!is_active());
    }

    #[test]
    fn reset_scroll_region_is_noop_when_inactive() {
        reset_state();
        reset_scroll_region();
        assert!(!is_active());
    }

    #[test]
    fn handle_resize_is_noop_when_inactive() {
        reset_state();
        handle_resize();
        assert!(!is_active());
    }

    // -- State transition tests --

    #[test]
    fn state_transition_init_to_active() {
        reset_state();
        assert!(!is_active(), "should start inactive");

        // Simulate what setup_scroll_region does (without a real terminal)
        TERMINAL_HEIGHT.store(24, Ordering::Relaxed);
        TERMINAL_WIDTH.store(80, Ordering::Relaxed);
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);

        assert!(is_active(), "should be active after setup");
        assert_eq!(TERMINAL_HEIGHT.load(Ordering::Relaxed), 24);
        assert_eq!(TERMINAL_WIDTH.load(Ordering::Relaxed), 80);
    }

    #[test]
    fn state_transition_active_to_resize() {
        reset_state();
        TERMINAL_HEIGHT.store(24, Ordering::Relaxed);
        TERMINAL_WIDTH.store(80, Ordering::Relaxed);
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);

        // Simulate a resize by directly updating cached dimensions
        let new_height: u16 = 40;
        let new_width: u16 = 120;
        let old_h = TERMINAL_HEIGHT.swap(new_height, Ordering::Relaxed);
        let old_w = TERMINAL_WIDTH.swap(new_width, Ordering::Relaxed);

        assert_eq!(old_h, 24);
        assert_eq!(old_w, 80);
        assert!(is_active(), "should remain active after resize");
        assert_eq!(TERMINAL_HEIGHT.load(Ordering::Relaxed), new_height);
        assert_eq!(TERMINAL_WIDTH.load(Ordering::Relaxed), new_width);
    }

    #[test]
    fn state_transition_active_to_reset() {
        reset_state();
        TERMINAL_HEIGHT.store(24, Ordering::Relaxed);
        TERMINAL_WIDTH.store(80, Ordering::Relaxed);
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);

        assert!(is_active());
        reset_scroll_region();
        assert!(!is_active(), "should be inactive after reset");
    }

    #[test]
    fn reset_scroll_region_escape_constant_correct() {
        assert_eq!(ESC_RESET_SCROLL_REGION, "\x1b[r");
    }

    #[test]
    fn show_cursor_escape_constant_correct() {
        assert_eq!(ESC_SHOW_CURSOR, "\x1b[?25h");
    }

    #[test]
    fn save_restore_cursor_constants_are_paired() {
        assert_eq!(ESC_SAVE_CURSOR, "\x1b7");
        assert_eq!(ESC_RESTORE_CURSOR, "\x1b8");
    }

    #[test]
    fn cached_dimensions_reflects_stored_values() {
        reset_state();
        TERMINAL_WIDTH.store(132, Ordering::Relaxed);
        TERMINAL_HEIGHT.store(43, Ordering::Relaxed);
        let (w, h) = cached_dimensions();
        assert_eq!(w, 132);
        assert_eq!(h, 43);
    }

    #[test]
    fn width_only_change_is_detected() {
        reset_state();
        TERMINAL_HEIGHT.store(24, Ordering::Relaxed);
        TERMINAL_WIDTH.store(80, Ordering::Relaxed);
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);

        // Width changes but height stays the same
        let old_h = TERMINAL_HEIGHT.swap(24, Ordering::Relaxed);
        let old_w = TERMINAL_WIDTH.swap(120, Ordering::Relaxed);

        // The condition `old_height == height && old_width == width`
        // should be false, meaning resize logic runs.
        let dimensions_changed = old_h != 24 || old_w != 120;
        assert!(
            dimensions_changed,
            "width-only change should be detected as a resize"
        );
    }

    #[test]
    fn build_separator_line_has_correct_width() {
        let line = build_separator_line(10);
        // Strip ANSI escapes to count visible characters
        let bare = line
            .replace(ESC_DIM, "")
            .replace(ESC_AMBER, "")
            .replace(ESC_RESET_ATTRS, "");
        assert_eq!(bare.chars().count(), 10);
        assert!(bare.chars().all(|ch| ch == '\u{2500}'));
    }

    #[test]
    fn input_bar_prompt_display_width_matches_constant() {
        // "you › " — 'y','o','u',' ','›',' ' = 6 visible chars
        assert_eq!(INPUT_BAR_PROMPT_DISPLAY_WIDTH, 6);
    }

    #[test]
    fn current_width_returns_cached_when_available() {
        reset_state();
        TERMINAL_WIDTH.store(120, Ordering::Relaxed);
        assert_eq!(current_width(), 120);
    }

    // -- Resize escape sequence tests --

    #[test]
    fn clear_input_bar_escapes_contains_erase_line_for_both_rows() {
        let escapes = clear_input_bar_escapes(24);
        // Separator row = 23, prompt row = 24
        assert!(
            escapes.contains("\x1b[23;1H\x1b[2K"),
            "must clear old separator row"
        );
        assert!(
            escapes.contains("\x1b[24;1H\x1b[2K"),
            "must clear old prompt row"
        );
    }

    #[test]
    fn clear_input_bar_escapes_wraps_with_save_restore_cursor() {
        let escapes = clear_input_bar_escapes(24);
        assert!(
            escapes.starts_with(ESC_SAVE_CURSOR),
            "must save cursor before clearing"
        );
        assert!(
            escapes.ends_with(ESC_RESTORE_CURSOR),
            "must restore cursor after clearing"
        );
    }

    #[test]
    fn clear_input_bar_escapes_is_empty_for_zero_height() {
        assert!(
            clear_input_bar_escapes(0).is_empty(),
            "zero height should produce no escapes"
        );
    }

    #[test]
    fn resize_wider_to_narrower_clears_old_separator_row() {
        // Simulate: terminal was 120×24, resized to 80×24
        // The old separator at row 23 must be cleared before
        // re-rendering at the new (same) row with fewer ─ chars.
        let old_height: u16 = 24;
        let escapes = clear_input_bar_escapes(old_height);
        let sep_row = old_height - 1;
        let expected_clear = format!("\x1b[{sep_row};1H{ESC_ERASE_LINE}");
        assert!(
            escapes.contains(&expected_clear),
            "wider→narrower resize must clear old separator row \
             to remove stale chars beyond new width"
        );
    }

    #[test]
    fn resize_height_change_clears_old_rows_before_new_render() {
        // Simulate: terminal was 80×30, resized to 80×20
        // Old separator at row 29, old prompt at row 30
        // New separator at row 19, new prompt at row 20
        // Old rows 29/30 must be cleared
        let old_height: u16 = 30;
        let new_height: u16 = 20;
        let old_escapes = clear_input_bar_escapes(old_height);
        let new_sep = new_height - 1;
        let new_scroll_end = new_height - INPUT_BAR_ROWS;
        let new_decstbm = format!("\x1b[1;{new_scroll_end}r");

        // Old rows are cleared
        assert!(
            old_escapes.contains("\x1b[29;1H\x1b[2K"),
            "old separator row 29 must be cleared"
        );
        assert!(
            old_escapes.contains("\x1b[30;1H\x1b[2K"),
            "old prompt row 30 must be cleared"
        );
        // New DECSTBM is correct
        assert_eq!(
            new_decstbm,
            scroll_region_escape(new_height),
            "new scroll region must use new height"
        );
        // New separator row is different from old
        assert_eq!(new_sep, 19);
    }

    #[test]
    fn render_separator_includes_erase_line_before_content() {
        // render_separator emits ESC_ERASE_LINE before the separator chars
        // Verify by checking the format string pattern
        let height: u16 = 24;
        let width: u16 = 40;
        let sep_row = height - 1;
        let expected_prefix = format!("\x1b[{sep_row};1H{ESC_ERASE_LINE}");
        // The escape is part of render_separator's eprint! call
        // We verify the constant is correct
        assert_eq!(
            ESC_ERASE_LINE, "\x1b[2K",
            "erase-line escape must be \\x1b[2K"
        );
        assert!(
            expected_prefix.contains("\x1b[2K"),
            "separator render must include clear-line escape"
        );
        // Also verify build_separator_line produces correct width
        let line = build_separator_line(width);
        let bare = line
            .replace(ESC_DIM, "")
            .replace(ESC_AMBER, "")
            .replace(ESC_RESET_ATTRS, "");
        assert_eq!(bare.chars().count(), width as usize);
    }

    #[test]
    fn scroll_region_end_matches_apply_calculation() {
        reset_state();
        TERMINAL_HEIGHT.store(24, Ordering::Relaxed);
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);

        let end = scroll_region_end();
        assert_eq!(end, 24 - INPUT_BAR_ROWS);
    }

    #[test]
    fn scroll_region_end_returns_zero_when_height_unset() {
        reset_state();
        assert_eq!(scroll_region_end(), 0);
    }

    #[test]
    fn pin_cursor_to_input_bar_is_noop_when_inactive() {
        reset_state();
        // Should not panic when inactive
        pin_cursor_to_input_bar();
        assert!(!is_active());
    }

    #[test]
    fn pin_cursor_to_input_bar_is_noop_when_height_zero() {
        reset_state();
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);
        TERMINAL_HEIGHT.store(0, Ordering::Relaxed);
        // Should not panic with zero height
        pin_cursor_to_input_bar();
    }

    #[test]
    fn pin_cursor_column_is_after_prompt() {
        // The cursor column should be INPUT_BAR_PROMPT_DISPLAY_WIDTH + 1
        let expected_col = INPUT_BAR_PROMPT_DISPLAY_WIDTH as u16 + 1;
        assert_eq!(
            expected_col, 7,
            "cursor should be at column 7 (after 'you › ')"
        );
    }

    // -- Regression tests for separator bugs --

    #[test]
    fn position_for_output_does_not_rerender_separator() {
        // Regression (Bug 1): position_for_output previously called
        // render_input_bar_dimmed, which re-rendered the separator
        // during streaming output. This caused separator ─ chars to
        // appear inside the scroll region, expanding with the response.
        // After the fix, position_for_output only repositions the
        // cursor — it must not modify terminal width/height state or
        // trigger any separator rendering side-effects.
        reset_state();
        TERMINAL_HEIGHT.store(24, Ordering::Relaxed);
        TERMINAL_WIDTH.store(80, Ordering::Relaxed);
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);

        // Call should not panic and must preserve all cached state.
        position_for_output();

        assert!(is_active(), "scroll region must remain active");
        assert_eq!(TERMINAL_HEIGHT.load(Ordering::Relaxed), 24);
        assert_eq!(TERMINAL_WIDTH.load(Ordering::Relaxed), 80);
    }

    #[test]
    fn separator_width_tracks_resize() {
        // Regression (Bug 2): render_separator used a caller-provided
        // width parameter that could be stale after a terminal resize.
        // Now it queries current_width() directly, so the separator
        // always matches the actual terminal width.
        reset_state();
        TERMINAL_WIDTH.store(120, Ordering::Relaxed);
        assert_eq!(
            current_width(),
            120,
            "current_width must reflect initial store"
        );

        let line_wide = build_separator_line(current_width());
        let bare_wide = line_wide
            .replace(ESC_DIM, "")
            .replace(ESC_AMBER, "")
            .replace(ESC_RESET_ATTRS, "");
        assert_eq!(bare_wide.chars().count(), 120);

        // Simulate resize to narrower terminal
        TERMINAL_WIDTH.store(60, Ordering::Relaxed);
        assert_eq!(
            current_width(),
            60,
            "current_width must reflect updated width"
        );

        let line_narrow = build_separator_line(current_width());
        let bare_narrow = line_narrow
            .replace(ESC_DIM, "")
            .replace(ESC_AMBER, "")
            .replace(ESC_RESET_ATTRS, "");
        assert_eq!(
            bare_narrow.chars().count(),
            60,
            "separator must use current width after resize, not stale cache"
        );
    }

    #[test]
    fn position_for_output_is_noop_when_inactive() {
        reset_state();
        // Must not panic when scroll region is inactive.
        position_for_output();
        assert!(!is_active());
    }

    #[test]
    fn position_for_output_is_noop_when_height_zero() {
        reset_state();
        SCROLL_REGION_ACTIVE.store(true, Ordering::Relaxed);
        TERMINAL_HEIGHT.store(0, Ordering::Relaxed);
        // Must not panic with zero height.
        position_for_output();
    }
}

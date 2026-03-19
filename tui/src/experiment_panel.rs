use std::time::{Duration, Instant};

const MAX_LINES: usize = 50;
const AUTO_HIDE_DELAY: Duration = Duration::from_secs(3);

/// Side panel showing live experiment progress in the TUI.
pub struct ExperimentPanel {
    lines: Vec<String>,
    visible: bool,
    auto_hide_at: Option<Instant>,
}

impl Default for ExperimentPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ExperimentPanel {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            visible: false,
            auto_hide_at: None,
        }
    }

    pub fn push_line(&mut self, line: String) {
        self.visible = true;
        self.auto_hide_at = None;
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            self.lines.drain(..self.lines.len() - MAX_LINES);
        }
    }

    pub fn mark_complete(&mut self) {
        self.auto_hide_at = Some(Instant::now() + AUTO_HIDE_DELAY);
    }

    pub fn check_auto_hide(&mut self) -> bool {
        if let Some(at) = self.auto_hide_at {
            if Instant::now() >= at {
                self.visible = false;
                self.auto_hide_at = None;
                return true;
            }
        }
        false
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.visible = false;
        self.auto_hide_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_panel_is_hidden() {
        let panel = ExperimentPanel::new();
        assert!(!panel.is_visible());
        assert!(panel.lines().is_empty());
    }

    #[test]
    fn push_line_makes_visible() {
        let mut panel = ExperimentPanel::new();
        panel.push_line("▸ Starting...".into());
        assert!(panel.is_visible());
        assert_eq!(panel.lines().len(), 1);
    }

    #[test]
    fn line_limit_enforced() {
        let mut panel = ExperimentPanel::new();
        for i in 0..60 {
            panel.push_line(format!("line {i}"));
        }
        assert_eq!(panel.lines().len(), MAX_LINES);
        assert_eq!(panel.lines()[0], "line 10");
    }

    #[test]
    fn clear_empties_lines() {
        let mut panel = ExperimentPanel::new();
        panel.push_line("test".into());
        panel.clear();
        assert!(panel.lines().is_empty());
    }

    #[test]
    fn check_auto_hide_before_timeout() {
        let mut panel = ExperimentPanel::new();
        panel.push_line("test".into());
        panel.mark_complete();
        // Immediately after mark_complete, should not hide yet
        assert!(!panel.check_auto_hide());
        assert!(panel.is_visible());
    }

    #[test]
    fn mark_complete_sets_auto_hide() {
        let mut panel = ExperimentPanel::new();
        panel.push_line("test".into());
        panel.mark_complete();
        assert!(panel.auto_hide_at.is_some());
    }

    #[test]
    fn check_auto_hide_fires_after_timeout() {
        let mut panel = ExperimentPanel::new();
        panel.push_line("test".into());
        // Set auto_hide_at to the past so it fires immediately
        panel.auto_hide_at = Some(Instant::now() - Duration::from_secs(1));
        assert!(panel.check_auto_hide());
        assert!(!panel.is_visible());
        assert!(panel.auto_hide_at.is_none());
    }

    #[test]
    fn push_line_cancels_auto_hide() {
        let mut panel = ExperimentPanel::new();
        panel.push_line("first".into());
        panel.mark_complete();
        assert!(panel.auto_hide_at.is_some());
        panel.push_line("second".into());
        assert!(panel.auto_hide_at.is_none());
    }

    #[test]
    fn clear_resets_visibility_and_timer() {
        let mut panel = ExperimentPanel::new();
        panel.push_line("test".into());
        panel.mark_complete();
        assert!(panel.is_visible());
        panel.clear();
        assert!(!panel.is_visible());
        assert!(panel.auto_hide_at.is_none());
        assert!(panel.lines().is_empty());
    }
}

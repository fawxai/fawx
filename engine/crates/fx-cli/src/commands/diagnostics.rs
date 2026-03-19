pub(crate) const CHECK_MARK: &str = "\x1b[32m✓\x1b[0m";
pub(crate) const WARNING_MARK: &str = "\x1b[33m⚠\x1b[0m";
pub(crate) const CROSS_MARK: &str = "\x1b[31m✗\x1b[0m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticStatus {
    Pass,
    Warning,
    NotConfigured,
    Fail,
}

impl DiagnosticStatus {
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Self::Pass => CHECK_MARK,
            Self::Warning => WARNING_MARK,
            Self::NotConfigured | Self::Fail => CROSS_MARK,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticLine {
    pub(crate) status: DiagnosticStatus,
    pub(crate) message: String,
}

impl DiagnosticLine {
    pub(crate) fn new(status: DiagnosticStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub(crate) fn print(&self) {
        println!("  {} {}", self.status.marker(), self.message);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticSection {
    pub(crate) title: &'static str,
    pub(crate) status: DiagnosticStatus,
    pub(crate) lines: Vec<DiagnosticLine>,
    pub(crate) footer: Option<String>,
}

impl DiagnosticSection {
    pub(crate) fn new(title: &'static str, lines: Vec<DiagnosticLine>) -> Self {
        Self::with_footer(title, lines, None)
    }

    pub(crate) fn with_footer(
        title: &'static str,
        lines: Vec<DiagnosticLine>,
        footer: Option<String>,
    ) -> Self {
        Self {
            title,
            status: combine_statuses_from_lines(&lines),
            lines,
            footer,
        }
    }

    pub(crate) fn print(&self) {
        println!("{}:", self.title);
        for line in &self.lines {
            line.print();
        }
        if let Some(footer) = &self.footer {
            println!("  {footer}");
        }
        println!();
    }
}

pub(crate) fn combine_statuses(statuses: &[DiagnosticStatus]) -> DiagnosticStatus {
    if statuses.contains(&DiagnosticStatus::Fail) {
        return DiagnosticStatus::Fail;
    }
    if statuses.contains(&DiagnosticStatus::Warning) {
        return DiagnosticStatus::Warning;
    }
    if statuses.contains(&DiagnosticStatus::NotConfigured) {
        return DiagnosticStatus::NotConfigured;
    }
    DiagnosticStatus::Pass
}

pub(crate) fn combine_statuses_from_lines(lines: &[DiagnosticLine]) -> DiagnosticStatus {
    let statuses = lines.iter().map(|line| line.status).collect::<Vec<_>>();
    combine_statuses(&statuses)
}

pub(crate) fn plural_suffix(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

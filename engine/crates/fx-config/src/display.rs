//! String conversion helpers for config enums and presets.

use crate::{BorrowScope, PermissionAction, PermissionPreset, ThinkingBudget};
use std::fmt;
use std::str::FromStr;

impl PermissionPreset {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Power => "power",
            Self::Cautious => "cautious",
            Self::Experimental => "experimental",
            Self::Custom => "custom",
        }
    }
}

impl FromStr for PermissionPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "power" | "standard" => Ok(Self::Power),
            "cautious" | "restricted" => Ok(Self::Cautious),
            "experimental" | "open" => Ok(Self::Experimental),
            "custom" => Ok(Self::Custom),
            other => Err(format!(
                "unknown permission preset '{other}'; expected power, cautious, experimental, custom, standard, restricted, open"
            )),
        }
    }
}

impl PermissionAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadAny => "read_any",
            Self::WebSearch => "web_search",
            Self::WebFetch => "web_fetch",
            Self::CodeExecute => "code_execute",
            Self::FileWrite => "file_write",
            Self::Git => "git",
            Self::Shell => "shell",
            Self::ToolCall => "tool_call",
            Self::SelfModify => "self_modify",
            Self::CredentialChange => "credential_change",
            Self::SystemInstall => "system_install",
            Self::NetworkListen => "network_listen",
            Self::OutboundMessage => "outbound_message",
            Self::FileDelete => "file_delete",
            Self::OutsideWorkspace => "outside_workspace",
            Self::KernelModify => "kernel_modify",
        }
    }
}

impl fmt::Display for ThinkingBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Adaptive => write!(f, "adaptive"),
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
            Self::Off => write!(f, "off"),
            Self::None => write!(f, "none"),
            Self::Minimal => write!(f, "minimal"),
            Self::Max => write!(f, "max"),
            Self::Xhigh => write!(f, "xhigh"),
        }
    }
}

impl ThinkingBudget {
    /// Map a budget level to its token count, or `None` for disabled variants.
    pub fn budget_tokens(&self) -> Option<u32> {
        match self {
            Self::Xhigh | Self::Max => Some(32_000),
            Self::High => Some(10_000),
            Self::Adaptive | Self::Medium => Some(5_000),
            Self::Low | Self::Minimal => Some(1_024),
            Self::Off | Self::None => Option::None,
        }
    }
}

impl FromStr for ThinkingBudget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "adaptive" => Ok(Self::Adaptive),
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            "off" => Ok(Self::Off),
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "max" => Ok(Self::Max),
            "xhigh" => Ok(Self::Xhigh),
            other => Err(format!(
                "unknown thinking level '{other}'; expected off, none, minimal, low, medium, high, xhigh, max, or adaptive"
            )),
        }
    }
}

impl fmt::Display for BorrowScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly => write!(f, "read_only"),
            Self::Contribution => write!(f, "contribution"),
        }
    }
}

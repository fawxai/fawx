//! Permission preset construction for common autonomy policies.

use crate::{CapabilityMode, PermissionAction, PermissionPreset, PermissionsConfig};
use std::str::FromStr;

impl PermissionsConfig {
    /// 🔥 Power User - full workspace autonomy, proposals for external actions.
    pub fn power() -> Self {
        Self {
            preset: PermissionPreset::Power,
            mode: CapabilityMode::Capability,
            unrestricted: actions(&[
                PermissionAction::ReadAny,
                PermissionAction::WebSearch,
                PermissionAction::WebFetch,
                PermissionAction::CodeExecute,
                PermissionAction::FileWrite,
                PermissionAction::Git,
                PermissionAction::Shell,
                PermissionAction::ToolCall,
                PermissionAction::SelfModify,
            ]),
            proposal_required: actions(&[
                PermissionAction::CredentialChange,
                PermissionAction::SystemInstall,
                PermissionAction::NetworkListen,
                PermissionAction::OutboundMessage,
                PermissionAction::FileDelete,
                PermissionAction::OutsideWorkspace,
                PermissionAction::KernelModify,
            ]),
        }
    }

    /// 🔒 Cautious - proposals for writes too.
    pub fn cautious() -> Self {
        Self {
            preset: PermissionPreset::Cautious,
            mode: CapabilityMode::Capability,
            unrestricted: actions(&[
                PermissionAction::ReadAny,
                PermissionAction::WebSearch,
                PermissionAction::WebFetch,
                PermissionAction::ToolCall,
            ]),
            proposal_required: actions(&[
                PermissionAction::CodeExecute,
                PermissionAction::FileWrite,
                PermissionAction::Git,
                PermissionAction::Shell,
                PermissionAction::SelfModify,
                PermissionAction::CredentialChange,
                PermissionAction::SystemInstall,
                PermissionAction::NetworkListen,
                PermissionAction::OutboundMessage,
                PermissionAction::FileDelete,
                PermissionAction::OutsideWorkspace,
                PermissionAction::KernelModify,
            ]),
        }
    }

    /// 🧪 Experimental - maximum autonomy including kernel self-modification.
    pub fn experimental() -> Self {
        Self {
            preset: PermissionPreset::Experimental,
            mode: CapabilityMode::Capability,
            unrestricted: actions(&[
                PermissionAction::ReadAny,
                PermissionAction::WebSearch,
                PermissionAction::WebFetch,
                PermissionAction::CodeExecute,
                PermissionAction::FileWrite,
                PermissionAction::Git,
                PermissionAction::Shell,
                PermissionAction::ToolCall,
                PermissionAction::SelfModify,
                PermissionAction::KernelModify,
            ]),
            proposal_required: actions(&[
                PermissionAction::CredentialChange,
                PermissionAction::SystemInstall,
                PermissionAction::NetworkListen,
                PermissionAction::OutboundMessage,
                PermissionAction::FileDelete,
                PermissionAction::OutsideWorkspace,
            ]),
        }
    }

    /// Open - everything allowed except privilege escalation.
    pub fn open() -> Self {
        Self {
            preset: PermissionPreset::Experimental,
            mode: CapabilityMode::Capability,
            ..Self::experimental()
        }
    }

    /// Standard - developer workflow, credential/system changes blocked.
    pub fn standard() -> Self {
        Self {
            preset: PermissionPreset::Power,
            mode: CapabilityMode::Capability,
            ..Self::power()
        }
    }

    /// Restricted - read-heavy, most writes blocked.
    pub fn restricted() -> Self {
        Self {
            preset: PermissionPreset::Cautious,
            mode: CapabilityMode::Capability,
            ..Self::cautious()
        }
    }

    pub fn from_preset_name(name: &str) -> Result<Self, String> {
        match PermissionPreset::from_str(name)? {
            PermissionPreset::Power => Ok(Self::power()),
            PermissionPreset::Cautious => Ok(Self::cautious()),
            PermissionPreset::Experimental => Ok(Self::experimental()),
            PermissionPreset::Custom => Ok(Self {
                preset: PermissionPreset::Custom,
                ..Self::default()
            }),
        }
    }
}

fn actions(list: &[PermissionAction]) -> Vec<PermissionAction> {
    list.to_vec()
}

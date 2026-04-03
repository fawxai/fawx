use crate::runtime_info::{AuthorityRuntimeInfo, SkillInfo};
use fx_config::{CapabilityMode, PermissionsConfig, SandboxConfig};
use serde::Serialize;

/// Machine-readable snapshot of the kernel's current configuration,
/// capabilities, and boundaries. Surfaces Layer 1 (capabilities) and
/// Layer 3 (hard boundaries). Deliberately excludes Layer 2
/// (tripwire/ripcord) per the AX invisibility invariant.
#[derive(Debug, Clone, Serialize)]
pub struct KernelManifest {
    pub version: String,
    pub preset: Option<String>,
    pub model: ModelInfo,
    pub permissions: PermissionManifest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<AuthorityManifest>,
    pub budget: BudgetManifest,
    pub sandbox: SandboxManifest,
    pub self_modify: SelfModifyManifest,
    pub tools: Vec<SkillManifest>,
    pub workspace: WorkspaceManifest,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub active_model: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionManifest {
    pub mode: String,
    pub unrestricted: Vec<String>,
    pub restricted: Vec<String>,
    pub default_policy: String,
    pub can_request_capabilities: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorityManifest {
    pub resolver: String,
    pub approval_scope: String,
    pub path_policy_source: String,
    pub capability_mode_mutates_path_policy: bool,
    pub kernel_blind_enabled: bool,
    pub sovereign_boundary_enforced: bool,
    pub active_session_approvals: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_proposal_override: Option<String>,
    pub recent_decisions: Vec<AuthorityDecisionManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorityDecisionManifest {
    pub tool_name: String,
    pub capability: String,
    pub effect: String,
    pub target_kind: String,
    pub domain: String,
    pub target_summary: String,
    pub verdict: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetManifest {
    pub max_llm_calls: u32,
    pub max_tool_invocations: u32,
    pub max_tokens: u64,
    pub max_wall_time_seconds: u64,
    pub max_retries_per_tool: u32,
    pub max_fan_out: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SandboxManifest {
    pub allow_network: bool,
    pub allow_subprocess: bool,
    pub max_execution_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelfModifyManifest {
    pub enabled: bool,
    pub allow_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillManifest {
    pub name: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceManifest {
    pub working_dir: String,
    pub writable_roots: Vec<String>,
}

/// Budget data extracted from `fx-kernel` without introducing a crate cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetSummary {
    pub max_llm_calls: u32,
    pub max_tool_invocations: u32,
    pub max_tokens: u64,
    pub max_wall_time_seconds: u64,
    pub max_retries_per_tool: u32,
    pub max_fan_out: usize,
}

pub struct ManifestSources<'a> {
    pub version: &'a str,
    pub active_model: &'a str,
    pub provider: &'a str,
    pub preset: Option<&'a str>,
    pub permissions: &'a PermissionsConfig,
    pub authority: Option<&'a AuthorityRuntimeInfo>,
    pub budget: &'a BudgetSummary,
    pub sandbox: &'a SandboxConfig,
    pub self_modify_enabled: bool,
    pub self_modify_allow: &'a [String],
    pub self_modify_deny: &'a [String],
    pub skills: &'a [SkillInfo],
    pub working_dir: &'a str,
    pub can_request_capabilities: bool,
}

#[must_use]
pub fn build_kernel_manifest(sources: &ManifestSources<'_>) -> KernelManifest {
    KernelManifest {
        version: sources.version.to_string(),
        preset: sources.preset.map(str::to_string),
        model: ModelInfo {
            active_model: sources.active_model.to_string(),
            provider: sources.provider.to_string(),
        },
        permissions: build_permission_manifest(
            sources.permissions,
            sources.can_request_capabilities,
        ),
        authority: sources.authority.map(build_authority_manifest),
        budget: build_budget_manifest(sources.budget),
        sandbox: SandboxManifest {
            allow_network: sources.sandbox.allow_network,
            allow_subprocess: sources.sandbox.allow_subprocess,
            max_execution_seconds: sources.sandbox.max_execution_seconds,
        },
        self_modify: SelfModifyManifest {
            enabled: sources.self_modify_enabled,
            allow_paths: sources.self_modify_allow.to_vec(),
            deny_paths: sources.self_modify_deny.to_vec(),
        },
        tools: sources
            .skills
            .iter()
            .map(|skill| SkillManifest {
                name: skill.name.clone(),
                tools: skill.tool_names.clone(),
            })
            .collect(),
        workspace: WorkspaceManifest {
            working_dir: sources.working_dir.to_string(),
            writable_roots: build_writable_roots(sources.working_dir, sources.self_modify_allow),
        },
    }
}

fn build_permission_manifest(config: &PermissionsConfig, can_request: bool) -> PermissionManifest {
    let mode = match config.mode {
        CapabilityMode::Capability => "capability",
        CapabilityMode::Prompt => "prompt",
    };
    PermissionManifest {
        mode: mode.to_string(),
        unrestricted: config
            .unrestricted
            .iter()
            .map(|action| action.as_str().to_string())
            .collect(),
        restricted: config
            .proposal_required
            .iter()
            .map(|action| action.as_str().to_string())
            .collect(),
        default_policy: if config.mode == CapabilityMode::Capability {
            "allow"
        } else {
            "ask"
        }
        .to_string(),
        can_request_capabilities: can_request,
    }
}

fn build_authority_manifest(info: &AuthorityRuntimeInfo) -> AuthorityManifest {
    AuthorityManifest {
        resolver: info.resolver.clone(),
        approval_scope: info.approval_scope.clone(),
        path_policy_source: info.path_policy_source.clone(),
        capability_mode_mutates_path_policy: info.capability_mode_mutates_path_policy,
        kernel_blind_enabled: info.kernel_blind_enabled,
        sovereign_boundary_enforced: info.sovereign_boundary_enforced,
        active_session_approvals: info.active_session_approvals,
        active_proposal_override: info.active_proposal_override.clone(),
        recent_decisions: info
            .recent_decisions
            .iter()
            .map(|decision| AuthorityDecisionManifest {
                tool_name: decision.tool_name.clone(),
                capability: decision.capability.clone(),
                effect: decision.effect.clone(),
                target_kind: decision.target_kind.clone(),
                domain: decision.domain.clone(),
                target_summary: decision.target_summary.clone(),
                verdict: decision.verdict.clone(),
                reason: decision.reason.clone(),
            })
            .collect(),
    }
}

fn build_budget_manifest(config: &BudgetSummary) -> BudgetManifest {
    BudgetManifest {
        max_llm_calls: config.max_llm_calls,
        max_tool_invocations: config.max_tool_invocations,
        max_tokens: config.max_tokens,
        max_wall_time_seconds: config.max_wall_time_seconds,
        max_retries_per_tool: config.max_retries_per_tool,
        max_fan_out: config.max_fan_out,
    }
}

fn build_writable_roots(working_dir: &str, allow_paths: &[String]) -> Vec<String> {
    let mut roots = vec![working_dir.to_string()];
    for path in allow_paths {
        if !roots.contains(path) {
            roots.push(path.clone());
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::{PermissionAction, PermissionPreset};

    struct TestFixture {
        permissions: PermissionsConfig,
        budget: BudgetSummary,
        sandbox: SandboxConfig,
        self_modify_allow: Vec<String>,
        self_modify_deny: Vec<String>,
        skills: Vec<SkillInfo>,
    }

    impl TestFixture {
        fn sources(&self) -> ManifestSources<'_> {
            ManifestSources {
                version: "1.2.3",
                active_model: "openai-codex/gpt-5.4",
                provider: "openai",
                preset: Some("power"),
                permissions: &self.permissions,
                authority: None,
                budget: &self.budget,
                sandbox: &self.sandbox,
                self_modify_enabled: true,
                self_modify_allow: &self.self_modify_allow,
                self_modify_deny: &self.self_modify_deny,
                skills: &self.skills,
                working_dir: "/workspace/fawx",
                can_request_capabilities: true,
            }
        }
    }

    fn test_permissions() -> PermissionsConfig {
        PermissionsConfig {
            preset: PermissionPreset::Power,
            mode: CapabilityMode::Capability,
            unrestricted: vec![PermissionAction::ReadAny, PermissionAction::WebSearch],
            proposal_required: vec![PermissionAction::Shell],
        }
    }

    fn test_fixture() -> TestFixture {
        TestFixture {
            permissions: test_permissions(),
            budget: BudgetSummary {
                max_llm_calls: 25,
                max_tool_invocations: 50,
                max_tokens: 12_000,
                max_wall_time_seconds: 180,
                max_retries_per_tool: 2,
                max_fan_out: 4,
            },
            sandbox: SandboxConfig {
                allow_network: true,
                allow_subprocess: false,
                max_execution_seconds: Some(30),
            },
            self_modify_allow: vec![
                "/workspace/fawx/docs".to_string(),
                "/workspace/fawx/scripts".to_string(),
            ],
            self_modify_deny: vec![".git/**".to_string(), "*.pem".to_string()],
            skills: vec![
                SkillInfo {
                    name: "builtin".to_string(),
                    description: Some("Built-in tools".to_string()),
                    tool_names: vec!["read_file".to_string(), "kernel_manifest".to_string()],
                    capabilities: Vec::new(),
                    version: None,
                    source: None,
                    revision_hash: None,
                    manifest_hash: None,
                    activated_at_ms: None,
                    signature_status: None,
                    stale_source: None,
                },
                SkillInfo {
                    name: "web".to_string(),
                    description: None,
                    tool_names: vec!["web_search".to_string()],
                    capabilities: vec!["search".to_string()],
                    version: None,
                    source: None,
                    revision_hash: None,
                    manifest_hash: None,
                    activated_at_ms: None,
                    signature_status: None,
                    stale_source: None,
                },
            ],
        }
    }

    #[test]
    fn build_kernel_manifest_includes_version() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert_eq!(manifest.version, "1.2.3");
    }

    #[test]
    fn build_kernel_manifest_uses_capability_mode_defaults() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert_eq!(manifest.permissions.mode, "capability");
        assert_eq!(manifest.permissions.default_policy, "allow");
    }

    #[test]
    fn build_kernel_manifest_uses_prompt_mode_defaults() {
        let mut fixture = test_fixture();
        fixture.permissions.mode = CapabilityMode::Prompt;
        let manifest = build_kernel_manifest(&fixture.sources());
        assert_eq!(manifest.permissions.mode, "prompt");
        assert_eq!(manifest.permissions.default_policy, "ask");
    }

    #[test]
    fn build_kernel_manifest_lists_restricted_categories() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert_eq!(manifest.permissions.restricted, vec!["shell"]);
    }

    #[test]
    fn build_kernel_manifest_includes_budget_limits() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert_eq!(manifest.budget.max_llm_calls, 25);
        assert_eq!(manifest.budget.max_tool_invocations, 50);
        assert_eq!(manifest.budget.max_tokens, 12_000);
        assert_eq!(manifest.budget.max_wall_time_seconds, 180);
        assert_eq!(manifest.budget.max_retries_per_tool, 2);
        assert_eq!(manifest.budget.max_fan_out, 4);
    }

    #[test]
    fn build_kernel_manifest_includes_sandbox() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert!(manifest.sandbox.allow_network);
        assert!(!manifest.sandbox.allow_subprocess);
        assert_eq!(manifest.sandbox.max_execution_seconds, Some(30));
    }

    #[test]
    fn build_kernel_manifest_includes_tools() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert_eq!(manifest.tools.len(), 2);
        assert_eq!(manifest.tools[0].name, "builtin");
        assert_eq!(
            manifest.tools[0].tools,
            vec!["read_file", "kernel_manifest"]
        );
    }

    #[test]
    fn build_kernel_manifest_serializes_to_json() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        let json = serde_json::to_value(&manifest).expect("serialize manifest");
        assert_eq!(json["model"]["active_model"], "openai-codex/gpt-5.4");
        assert_eq!(json["workspace"]["working_dir"], "/workspace/fawx");
    }

    #[test]
    fn empty_permission_policy_shows_no_restrictions() {
        let mut fixture = test_fixture();
        fixture.permissions.unrestricted.clear();
        fixture.permissions.proposal_required.clear();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert!(manifest.permissions.unrestricted.is_empty());
        assert!(manifest.permissions.restricted.is_empty());
    }

    #[test]
    fn build_kernel_manifest_excludes_tripwire_state() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        let json = serde_json::to_string(&manifest).expect("serialize manifest");
        assert!(!json.contains("tripwire"));
        assert!(!json.contains("ripcord"));
    }

    #[test]
    fn build_kernel_manifest_includes_preset_name() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert_eq!(manifest.preset.as_deref(), Some("power"));
    }

    #[test]
    fn build_kernel_manifest_includes_self_modify() {
        let fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert!(manifest.self_modify.enabled);
        assert_eq!(
            manifest.self_modify.allow_paths,
            vec!["/workspace/fawx/docs", "/workspace/fawx/scripts"]
        );
        assert_eq!(manifest.self_modify.deny_paths, vec![".git/**", "*.pem"]);
    }

    #[test]
    fn build_writable_roots_deduplicates_working_dir() {
        let roots = build_writable_roots(
            "/workspace/fawx",
            &["/workspace/fawx".to_string(), "/other".to_string()],
        );
        assert_eq!(roots, vec!["/workspace/fawx", "/other"]);
    }

    #[test]
    fn build_kernel_manifest_includes_escalation_flag() {
        let mut fixture = test_fixture();
        let manifest = build_kernel_manifest(&fixture.sources());
        assert!(manifest.permissions.can_request_capabilities);

        fixture.skills[0]
            .tool_names
            .retain(|name| name != "kernel_manifest");
        let sources = ManifestSources {
            can_request_capabilities: false,
            ..fixture.sources()
        };
        let manifest = build_kernel_manifest(&sources);
        assert!(!manifest.permissions.can_request_capabilities);
    }
}

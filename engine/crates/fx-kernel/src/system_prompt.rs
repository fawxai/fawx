use fx_config::AgentConfig;

const COMMON_BEHAVIORAL_RULES: &str = "\
Use tools when you need information not in the conversation. \
When the user's request matches an available tool, prefer calling the tool over answering from general knowledge. \
After using tools, respond with the answer directly. \
For multi-step tasks, use the decompose tool to break work into parallel sub-goals.";

const CASUAL_IDENTITY_TEMPLATE: &str = r#"You are {name}, a personal AI agent running on this machine.
You're direct, helpful, and concise. Skip the formalities and filler.
Answer questions naturally. When you use tools, just share the results
without narrating what you did or how you got them."#;
const PROFESSIONAL_IDENTITY_TEMPLATE: &str = r#"You are {name}, a personal AI agent running on this machine.
Provide clear, structured responses. Be thorough but efficient.
Use tools when needed and present results directly without narrating the process."#;
const TECHNICAL_IDENTITY_TEMPLATE: &str = r#"You are {name}, a personal AI agent running on this machine.
Be precise and terse. No filler, no hedging. Use tools, report results.
Code and data speak louder than prose."#;
const MINIMAL_IDENTITY_TEMPLATE: &str = r#"You are {name}. Answer with minimum words. No filler. Tools: use them, report results only."#;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Personality {
    #[default]
    Casual,
    Professional,
    Technical,
    Minimal,
    Custom(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Surface {
    NativeApp,
    #[default]
    Tui,
    HeadlessApi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityConfig {
    pub name: String,
    pub personality: Personality,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            name: "Fawx".to_string(),
            personality: Personality::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BehaviorConfig {
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityGroup {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SecurityMode {
    #[default]
    Capability,
    Prompt,
    Open,
}

impl SecurityMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Capability => "capability",
            Self::Prompt => "prompt",
            Self::Open => "open",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityContext {
    pub mode: SecurityMode,
    pub restricted: Vec<String>,
    pub working_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    pub is_new: bool,
    pub message_count: usize,
    pub recent_summary: Option<String>,
}

impl Default for SessionContext {
    fn default() -> Self {
        Self {
            is_new: true,
            message_count: 0,
            recent_summary: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemPromptBuilder {
    identity: IdentityConfig,
    behavior: BehaviorConfig,
    capabilities: Vec<CapabilityGroup>,
    security: Option<SecurityContext>,
    user_context: Option<String>,
    surface: Surface,
    session: SessionContext,
    directives: Vec<String>,
}

impl SystemPromptBuilder {
    pub fn new(identity: IdentityConfig, behavior: BehaviorConfig) -> Self {
        Self {
            identity,
            behavior,
            capabilities: Vec::new(),
            security: None,
            user_context: None,
            surface: Surface::default(),
            session: SessionContext::default(),
            directives: Vec::new(),
        }
    }

    pub fn from_config(agent: &AgentConfig) -> Self {
        Self::new(identity_from_config(agent), behavior_from_config(agent))
    }

    pub fn capabilities(mut self, capabilities: Vec<CapabilityGroup>) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn security(mut self, security: SecurityContext) -> Self {
        self.security = Some(security);
        self
    }

    pub fn user_context(mut self, user_context: impl Into<String>) -> Self {
        self.user_context = Some(user_context.into());
        self
    }

    pub fn surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    pub fn session(mut self, session: SessionContext) -> Self {
        self.session = session;
        self
    }

    pub fn directive(mut self, directive: impl Into<String>) -> Self {
        self.directives.push(directive.into());
        self
    }

    pub fn build(&self) -> String {
        [
            Some(render_identity(&self.identity)),
            render_behavior(&self.behavior),
            render_capabilities(&self.capabilities),
            self.security.as_ref().map(render_security),
            render_user_context(self.user_context.as_deref()),
            Some(render_surface(&self.surface)),
            Some(render_session(&self.session)),
            render_directives(&self.directives),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n")
    }
}

fn identity_from_config(agent: &AgentConfig) -> IdentityConfig {
    IdentityConfig {
        name: agent.name.clone(),
        personality: personality_from_config(agent),
    }
}

fn behavior_from_config(agent: &AgentConfig) -> BehaviorConfig {
    BehaviorConfig {
        custom_instructions: agent.behavior.custom_instructions.clone(),
    }
}

fn personality_from_config(agent: &AgentConfig) -> Personality {
    match agent.personality.trim().to_ascii_lowercase().as_str() {
        "casual" | "" => Personality::Casual,
        "professional" => Personality::Professional,
        "technical" => Personality::Technical,
        "minimal" => Personality::Minimal,
        _ => custom_personality(agent).unwrap_or_default(),
    }
}

fn custom_personality(agent: &AgentConfig) -> Option<Personality> {
    non_empty(agent.custom_personality.as_deref()).map(|text| Personality::Custom(text.to_string()))
}

fn render_identity(identity: &IdentityConfig) -> String {
    let base = match &identity.personality {
        Personality::Casual => CASUAL_IDENTITY_TEMPLATE.replace("{name}", &identity.name),
        Personality::Professional => {
            PROFESSIONAL_IDENTITY_TEMPLATE.replace("{name}", &identity.name)
        }
        Personality::Technical => TECHNICAL_IDENTITY_TEMPLATE.replace("{name}", &identity.name),
        Personality::Minimal => MINIMAL_IDENTITY_TEMPLATE.replace("{name}", &identity.name),
        Personality::Custom(text) => {
            format!(
                "You are {}, a personal AI agent running on this machine.\n{}",
                identity.name, text
            )
        }
    };
    format!("{base}\n{COMMON_BEHAVIORAL_RULES}")
}

fn render_behavior(behavior: &BehaviorConfig) -> Option<String> {
    let instructions = non_empty(behavior.custom_instructions.as_deref())?;
    Some(format!("Behavioral:\n{instructions}"))
}

fn render_capabilities(capabilities: &[CapabilityGroup]) -> Option<String> {
    if capabilities.is_empty() {
        return None;
    }

    let bullets = capabilities
        .iter()
        .map(|capability| format!("- {}: {}", capability.name, capability.description))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!("Capabilities:\n{bullets}"))
}

fn render_security(security: &SecurityContext) -> String {
    let restricted = if security.restricted.is_empty() {
        "none".to_string()
    } else {
        security.restricted.join(", ")
    };

    format!(
        "Security:\n- Mode: {}\n- Restricted: {}\n- Working directory: {}",
        security.mode.as_str(),
        restricted,
        security.working_dir
    )
}

fn render_user_context(user_context: Option<&str>) -> Option<String> {
    let user_context = non_empty(user_context)?;
    Some(format!("User context:\n{user_context}"))
}

fn render_surface(surface: &Surface) -> String {
    match surface {
        Surface::NativeApp => {
            "Surface: Native app. Assume app-style interaction and GUI affordances.".to_string()
        }
        Surface::Tui => "Surface: TUI. Keep responses scannable in a terminal.".to_string(),
        Surface::HeadlessApi => {
            "Surface: Headless API. Return plain content without UI-specific references."
                .to_string()
        }
    }
}

fn render_session(session: &SessionContext) -> String {
    let state = if session.is_new { "new" } else { "continuing" };
    let mut lines = vec![format!(
        "Session:\n- State: {state}\n- Message count: {}",
        session.message_count
    )];

    if let Some(summary) = non_empty(session.recent_summary.as_deref()) {
        lines.push(format!("- Recent summary: {summary}"));
    }

    lines.join("\n")
}

fn render_directives(directives: &[String]) -> Option<String> {
    let bullets = directives
        .iter()
        .filter_map(|directive| non_empty(Some(directive.as_str())))
        .map(|directive| format!("- {directive}"))
        .collect::<Vec<_>>();

    if bullets.is_empty() {
        return None;
    }

    Some(format!("Directives:\n{}", bullets.join("\n")))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .and_then(|trimmed| (!trimmed.is_empty()).then_some(trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::{AgentBehaviorConfig, AgentConfig};

    fn builder_with_personality(personality: Personality) -> SystemPromptBuilder {
        SystemPromptBuilder::new(
            IdentityConfig {
                name: "Rivet".to_string(),
                personality,
            },
            BehaviorConfig::default(),
        )
    }

    #[test]
    fn casual_personality_includes_name() {
        let prompt = builder_with_personality(Personality::Casual).build();

        assert!(prompt.contains("You are Rivet, a personal AI agent running on this machine."));
        assert!(prompt.contains("You're direct, helpful, and concise."));
        assert!(prompt.contains(COMMON_BEHAVIORAL_RULES));
    }

    #[test]
    fn professional_personality_output() {
        let prompt = builder_with_personality(Personality::Professional).build();

        assert!(prompt.contains("Provide clear, structured responses."));
        assert!(prompt.contains("Be thorough but efficient."));
    }

    #[test]
    fn technical_personality_output() {
        let prompt = builder_with_personality(Personality::Technical).build();

        assert!(prompt.contains("Be precise and terse."));
        assert!(prompt.contains("Code and data speak louder than prose."));
    }

    #[test]
    fn minimal_personality_output() {
        let prompt = builder_with_personality(Personality::Minimal).build();

        assert!(prompt.contains("You are Rivet. Answer with minimum words."));
        assert!(prompt.contains("Tools: use them, report results only."));
    }

    #[test]
    fn custom_personality_uses_text() {
        let prompt = builder_with_personality(Personality::Custom(
            "Be warm, pragmatic, and slightly opinionated.".to_string(),
        ))
        .build();

        assert!(prompt.contains("You are Rivet, a personal AI agent running on this machine."));
        assert!(prompt.contains("Be warm, pragmatic, and slightly opinionated."));
    }

    #[test]
    fn from_config_maps_custom_personality_and_behavior() {
        let prompt = SystemPromptBuilder::from_config(&AgentConfig {
            name: "Rivet".to_string(),
            personality: "custom".to_string(),
            custom_personality: Some("Be warm, pragmatic, and slightly opinionated.".to_string()),
            behavior: AgentBehaviorConfig {
                custom_instructions: Some("Prefer concrete next steps.".to_string()),
                verbosity: "thorough".to_string(),
                proactive: true,
            },
        })
        .build();

        assert!(prompt.contains("You are Rivet, a personal AI agent running on this machine."));
        assert!(prompt.contains("Be warm, pragmatic, and slightly opinionated."));
        assert!(prompt.contains("Behavioral:\nPrefer concrete next steps."));
    }

    #[test]
    fn from_config_falls_back_to_default_personality_when_custom_text_missing() {
        let prompt = SystemPromptBuilder::from_config(&AgentConfig {
            name: "Rivet".to_string(),
            personality: "custom".to_string(),
            custom_personality: Some("   ".to_string()),
            behavior: AgentBehaviorConfig::default(),
        })
        .build();

        assert!(prompt.contains("You're direct, helpful, and concise."));
    }

    #[test]
    fn build_assembles_all_layers() {
        let prompt = SystemPromptBuilder::new(
            IdentityConfig {
                name: "Rivet".to_string(),
                personality: Personality::Professional,
            },
            BehaviorConfig {
                custom_instructions: Some("Keep answers grounded in evidence.".to_string()),
            },
        )
        .capabilities(vec![CapabilityGroup {
            name: "web_fetch".to_string(),
            description: "Fetch a web page".to_string(),
        }])
        .security(SecurityContext {
            mode: SecurityMode::Capability,
            restricted: vec!["kernel_modify".to_string()],
            working_dir: "/workspace".to_string(),
        })
        .user_context("Joe prefers short answers.")
        .surface(Surface::HeadlessApi)
        .session(SessionContext {
            is_new: false,
            message_count: 3,
            recent_summary: Some("Reviewed deployment notes.".to_string()),
        })
        .directive("Return machine-readable content when asked.")
        .build();

        let expected = [
            "Behavioral:\nKeep answers grounded in evidence.",
            "Capabilities:\n- web_fetch: Fetch a web page",
            "Security:\n- Mode: capability\n- Restricted: kernel_modify\n- Working directory: /workspace",
            "User context:\nJoe prefers short answers.",
            "Surface: Headless API. Return plain content without UI-specific references.",
            "Session:\n- State: continuing\n- Message count: 3\n- Recent summary: Reviewed deployment notes.",
            "Directives:\n- Return machine-readable content when asked.",
        ]
        .join("\n\n");

        assert!(prompt.contains(&expected));
        assert!(!prompt.contains("\n\n\n"));
    }

    #[test]
    fn build_skips_none_layers() {
        let prompt = SystemPromptBuilder::new(
            IdentityConfig::default(),
            BehaviorConfig {
                custom_instructions: Some("   ".to_string()),
            },
        )
        .directive("   ")
        .build();

        assert!(!prompt.contains("Behavioral:"));
        assert!(!prompt.contains("Capabilities:"));
        assert!(!prompt.contains("Security:"));
        assert!(!prompt.contains("User context:"));
        assert!(!prompt.contains("Directives:"));
        assert!(!prompt.contains("\n\n\n"));
    }

    #[test]
    fn capabilities_render_as_list() {
        let prompt = builder_with_personality(Personality::Casual)
            .capabilities(vec![
                CapabilityGroup {
                    name: "web_search".to_string(),
                    description: "Search the public web".to_string(),
                },
                CapabilityGroup {
                    name: "read".to_string(),
                    description: "Read local files".to_string(),
                },
            ])
            .build();

        assert!(prompt.contains("Capabilities:"));
        assert!(prompt.contains("- web_search: Search the public web"));
        assert!(prompt.contains("- read: Read local files"));
    }

    #[test]
    fn security_context_renders_boundaries() {
        let prompt = builder_with_personality(Personality::Casual)
            .security(SecurityContext {
                mode: SecurityMode::Capability,
                restricted: vec!["network_listen".to_string(), "kernel_modify".to_string()],
                working_dir: "/workspace".to_string(),
            })
            .build();

        assert!(prompt.contains("Security:"));
        assert!(prompt.contains("- Mode: capability"));
        assert!(prompt.contains("- Restricted: network_listen, kernel_modify"));
        assert!(prompt.contains("- Working directory: /workspace"));
    }

    #[test]
    fn security_mode_variants_render_lowercase_labels() {
        for (mode, label) in [
            (SecurityMode::Capability, "capability"),
            (SecurityMode::Prompt, "prompt"),
            (SecurityMode::Open, "open"),
        ] {
            let prompt = builder_with_personality(Personality::Casual)
                .security(SecurityContext {
                    mode,
                    restricted: Vec::new(),
                    working_dir: "/workspace".to_string(),
                })
                .build();

            assert!(prompt.contains(&format!("- Mode: {label}")));
        }
    }

    #[test]
    fn session_new_vs_continuing() {
        let new_prompt = builder_with_personality(Personality::Casual)
            .session(SessionContext {
                is_new: true,
                message_count: 0,
                recent_summary: None,
            })
            .build();
        let continuing_prompt = builder_with_personality(Personality::Casual)
            .session(SessionContext {
                is_new: false,
                message_count: 12,
                recent_summary: Some("Reviewed the deployment plan.".to_string()),
            })
            .build();

        assert!(new_prompt.contains("Session:\n- State: new\n- Message count: 0"));
        assert!(continuing_prompt.contains("Session:\n- State: continuing\n- Message count: 12"));
        assert!(continuing_prompt.contains("- Recent summary: Reviewed the deployment plan."));
    }

    #[test]
    fn surface_variants() {
        let native = builder_with_personality(Personality::Casual)
            .surface(Surface::NativeApp)
            .build();
        let tui = builder_with_personality(Personality::Casual)
            .surface(Surface::Tui)
            .build();
        let headless = builder_with_personality(Personality::Casual)
            .surface(Surface::HeadlessApi)
            .build();

        assert!(native
            .contains("Surface: Native app. Assume app-style interaction and GUI affordances."));
        assert!(tui.contains("Surface: TUI. Keep responses scannable in a terminal."));
        assert!(headless.contains(
            "Surface: Headless API. Return plain content without UI-specific references."
        ));
    }

    #[test]
    fn custom_instructions_in_main_prompt() {
        let prompt = SystemPromptBuilder::new(
            IdentityConfig::default(),
            BehaviorConfig {
                custom_instructions: Some("Prefer concrete next steps.".to_string()),
            },
        )
        .build();

        assert!(prompt.contains("Behavioral:\nPrefer concrete next steps."));
    }

    #[test]
    fn directives_appended_at_end() {
        let prompt = builder_with_personality(Personality::Casual)
            .directive("Ask before destructive actions.")
            .directive("Confirm before sending messages.")
            .build();

        assert!(prompt.ends_with(
            "Directives:\n- Ask before destructive actions.\n- Confirm before sending messages."
        ));
    }
}

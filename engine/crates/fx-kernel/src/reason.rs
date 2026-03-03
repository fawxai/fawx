//! Reason-step prompt assembly and response parsing.

use crate::types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

/// Processes [`ReasoningContext`] and produces [`ReasonedIntent`] values.
///
/// The actual LLM call is delegated to `fx-llm`; this module handles prompt
/// assembly and response parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningEngine {
    /// Maximum intents per reasoning cycle.
    max_intents: usize,
    /// Minimum confidence to accept an intent.
    confidence_threshold: f32,
}

impl ReasoningEngine {
    /// Create a new [`ReasoningEngine`].
    pub fn new(max_intents: usize, confidence_threshold: f32) -> Self {
        Self {
            max_intents,
            confidence_threshold,
        }
    }

    /// Build the prompt that will be sent to the LLM.
    ///
    /// The resulting prompt includes a system instruction, a context-rich user
    /// message, and available tool definitions.
    pub fn build_prompt(&self, context: &ReasoningContext) -> ReasoningPrompt {
        let system = format!(
            "You are the Fawx kernel reasoning planner. Return ONLY JSON with shape \
             {{\"intents\":[{{\"action\":...,\"rationale\":\"...\",\"confidence\":0.0,\"expected_outcome\":null,\"sub_goals\":[]}}]}}. \
             Generate at most {} intents.",
            self.max_intents
        );

        let mut lines = Vec::new();
        lines.push(format!("Goal: {}", context.goal.description));
        lines.push(format!("Depth: {}", context.depth));
        lines.push(format!("Active app: {}", context.perception.active_app));
        lines.push(format!(
            "Screen text: {}",
            context.perception.screen.text_content
        ));

        if !context.goal.success_criteria.is_empty() {
            lines.push("Success criteria:".to_owned());
            for criterion in &context.goal.success_criteria {
                lines.push(format!("- {criterion}"));
            }
        }

        if !context.working_memory.is_empty() {
            lines.push("Working memory:".to_owned());
            for entry in &context.working_memory {
                lines.push(format!(
                    "- {} = {} (rel {:.2})",
                    entry.key, entry.value, entry.relevance
                ));
            }
        }

        if !context.relevant_episodic.is_empty() {
            lines.push("Relevant episodic memory:".to_owned());
            for memory in &context.relevant_episodic {
                lines.push(format!(
                    "- [{}] {} (rel {:.2})",
                    memory.id, memory.summary, memory.relevance
                ));
            }
        }

        if !context.relevant_semantic.is_empty() {
            lines.push("Relevant semantic memory:".to_owned());
            for fact in &context.relevant_semantic {
                lines.push(format!(
                    "- [{}] {} (conf {:.2})",
                    fact.id, fact.fact, fact.confidence
                ));
            }
        }

        if !context.active_procedures.is_empty() {
            lines.push("Available procedures:".to_owned());
            for procedure in &context.active_procedures {
                lines.push(format!(
                    "- {} ({}) v{}",
                    procedure.name, procedure.id, procedure.version
                ));
            }
        }

        if context.identity_context.user_name.is_some()
            || !context.identity_context.preferences.is_empty()
            || !context.identity_context.personality_traits.is_empty()
        {
            lines.push("Identity context:".to_owned());

            if let Some(user_name) = &context.identity_context.user_name {
                lines.push(format!("- User name: {user_name}"));
            }

            if !context.identity_context.preferences.is_empty() {
                lines.push("- Preferences:".to_owned());
                let mut preference_entries = context
                    .identity_context
                    .preferences
                    .iter()
                    .collect::<Vec<_>>();
                preference_entries.sort_by(|left, right| left.0.cmp(right.0));

                for (key, value) in preference_entries {
                    lines.push(format!("  - {key}: {value}"));
                }
            }

            if !context.identity_context.personality_traits.is_empty() {
                lines.push(format!(
                    "- Personality traits: {}",
                    context.identity_context.personality_traits.join(", ")
                ));
            }
        }

        if let Some(parent) = &context.parent_context {
            lines.push(format!(
                "Parent goal: {} (depth {})",
                parent.goal.description, parent.depth
            ));
        }

        let messages = vec![PromptMessage {
            role: PromptRole::User,
            content: lines.join("\n"),
        }];

        ReasoningPrompt {
            system,
            messages,
            tools: build_tool_definitions(context),
        }
    }

    /// Parse LLM response into [`ReasonedIntent`] values.
    ///
    /// Responses that fail JSON parsing are ignored. Parsed intents are filtered
    /// by the configured confidence threshold and capped to `max_intents`.
    pub fn parse_response(
        &self,
        raw_response: &str,
        _context: &ReasoningContext,
    ) -> Vec<ReasonedIntent> {
        let mut intents = parse_intents(raw_response.trim())
            .or_else(|| {
                extract_json_payload(raw_response)
                    .and_then(|payload| parse_intents(payload.as_str()))
            })
            .or_else(|| {
                extract_json_from_first_bracket(raw_response)
                    .and_then(|payload| parse_intents(payload.as_str()))
            })
            .unwrap_or_default();

        for intent in &mut intents {
            if intent.confidence.is_finite() {
                intent.confidence = intent.confidence.clamp(0.0, 1.0);
            }
        }

        intents.retain(|intent| {
            intent.confidence.is_finite() && intent.confidence >= self.confidence_threshold
        });

        intents.truncate(self.max_intents);
        intents
    }
}

/// A structured prompt ready to send to an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPrompt {
    /// System-level instruction for model behavior.
    pub system: String,
    /// Ordered prompt messages.
    pub messages: Vec<PromptMessage>,
    /// Declared tools/actions available to the model.
    pub tools: Vec<ToolDefinition>,
}

/// A single role-tagged message in a reasoning prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    /// Message role.
    pub role: PromptRole,
    /// Message text payload.
    pub content: String,
}

/// Role for prompt messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromptRole {
    /// System instruction role.
    System,
    /// End-user/context role.
    User,
    /// Assistant continuation role.
    Assistant,
}

/// Tool definition exposed to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool identifier.
    pub name: String,
    /// Human-readable tool description.
    pub description: String,
    /// JSON schema describing tool parameters.
    pub parameters_schema: serde_json::Value,
}

fn build_tool_definitions(context: &ReasoningContext) -> Vec<ToolDefinition> {
    let mut tools = vec![ToolDefinition {
        name: "emit_intent".to_owned(),
        description: "Emit one ReasonedIntent object for the next action.".to_owned(),
        parameters_schema: json!({
            "type": "object",
            "properties": {
                "action": { "type": "object", "description": "IntendedAction payload" },
                "rationale": { "type": "string" },
                "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                "expected_outcome": { "type": ["object", "null"] },
                "sub_goals": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" },
                            "success_criteria": { "type": "array", "items": { "type": "string" } },
                            "max_steps": { "type": ["integer", "null"] }
                        },
                        "required": ["description", "success_criteria"]
                    }
                }
            },
            "required": ["action", "rationale", "confidence", "sub_goals"]
        }),
    }];

    let mut seen_names = HashSet::new();
    seen_names.insert("emit_intent".to_owned());

    for procedure in &context.active_procedures {
        let tool_name = format!("procedure_{}", sanitize_tool_name(&procedure.id));
        if !seen_names.insert(tool_name.clone()) {
            continue;
        }

        tools.push(ToolDefinition {
            name: tool_name,
            description: format!(
                "Invoke procedure '{}' (version {}).",
                procedure.name, procedure.version
            ),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "arguments": {
                        "type": "object",
                        "description": "Procedure-specific arguments"
                    }
                },
                "additionalProperties": true
            }),
        });
    }

    tools
}

fn sanitize_tool_name(input: &str) -> String {
    let normalized: String = input
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();

    if normalized.is_empty() {
        "procedure".to_owned()
    } else {
        normalized
    }
}

fn extract_json_payload(raw_response: &str) -> Option<String> {
    extract_last_fenced_segment(raw_response, true)
        .or_else(|| extract_last_fenced_segment(raw_response, false))
}

fn extract_last_fenced_segment(raw_response: &str, json_only: bool) -> Option<String> {
    let mut segments = extract_fenced_segments(raw_response)
        .into_iter()
        .filter(|(_, language, payload)| {
            let is_json = language.eq_ignore_ascii_case("json");
            !payload.is_empty() && (is_json || !json_only)
        })
        .collect::<Vec<_>>();

    segments.pop().map(|(_, _, payload)| payload)
}

fn extract_fenced_segments(raw_response: &str) -> Vec<(usize, String, String)> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;

    while let Some(open_rel) = raw_response[cursor..].find("```") {
        let open_index = cursor + open_rel;
        let after_open = &raw_response[open_index + 3..];
        let Some(newline_rel) = after_open.find('\n') else {
            cursor = open_index + 3;
            continue;
        };

        let language = after_open[..newline_rel].trim().to_owned();
        let payload_start = open_index + 3 + newline_rel + 1;

        let Some(close_rel) = raw_response[payload_start..].find("```") else {
            cursor = open_index + 3;
            continue;
        };

        let payload_end = payload_start + close_rel;
        let payload = raw_response[payload_start..payload_end].trim().to_owned();

        segments.push((open_index, language, payload));
        cursor = payload_end + 3;
    }

    segments
}

fn extract_json_from_first_bracket(raw_response: &str) -> Option<String> {
    let first_bracket = raw_response.find(['{', '['])?;
    extract_balanced_json(&raw_response[first_bracket..])
        .or_else(|| Some(raw_response[first_bracket..].trim().to_owned()))
}

fn extract_balanced_json(candidate: &str) -> Option<String> {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in candidate.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }

            match character {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '{' | '[' => stack.push(character),
            '}' => {
                if stack.pop() != Some('{') {
                    return None;
                }
                if stack.is_empty() {
                    return Some(candidate[..index + character.len_utf8()].to_owned());
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return None;
                }
                if stack.is_empty() {
                    return Some(candidate[..index + character.len_utf8()].to_owned());
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_intents(payload: &str) -> Option<Vec<ReasonedIntent>> {
    #[derive(Debug, Deserialize)]
    struct Envelope {
        intents: Vec<ReasonedIntent>,
    }

    if let Ok(envelope) = serde_json::from_str::<Envelope>(payload) {
        return Some(envelope.intents);
    }

    if let Ok(intents) = serde_json::from_str::<Vec<ReasonedIntent>>(payload) {
        return Some(intents);
    }

    if let Ok(single) = serde_json::from_str::<ReasonedIntent>(payload) {
        return Some(vec![single]);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::types::ScreenState;
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_context() -> ReasoningContext {
        let mut preferences = HashMap::new();
        preferences.insert("tone".to_owned(), "direct".to_owned());

        ReasoningContext {
            perception: PerceptionSnapshot {
                screen: ScreenState {
                    current_app: "com.example.chat".to_owned(),
                    elements: vec![],
                    text_content: "Unread message from Alex".to_owned(),
                },
                notifications: vec![],
                active_app: "com.example.chat".to_owned(),
                timestamp_ms: 1_700_000_000_001,
                sensor_data: None,
                user_input: None,
                conversation_history: Vec::new(),
                steer_context: None,
            },
            working_memory: vec![WorkingMemoryEntry {
                key: "last_contact".to_owned(),
                value: "Alex".to_owned(),
                relevance: 0.9,
            }],
            relevant_episodic: vec![EpisodicMemoryRef {
                id: 7,
                summary: "Alex prefers concise replies".to_owned(),
                relevance: 0.8,
                timestamp_ms: 1_700_000_000_000,
            }],
            relevant_semantic: vec![SemanticMemoryRef {
                id: 9,
                fact: "Alex is in Pacific time".to_owned(),
                confidence: 0.75,
            }],
            active_procedures: vec![ProcedureRef {
                id: "reply-template".to_owned(),
                name: "Reply Template".to_owned(),
                version: 2,
            }],
            identity_context: IdentityContext {
                user_name: Some("Joe".to_owned()),
                preferences,
                personality_traits: vec!["helpful".to_owned()],
            },
            goal: Goal::new(
                "Draft and send a reply",
                vec!["Reply is sent".to_owned()],
                Some(3),
            ),
            depth: 0,
            parent_context: None,
        }
    }

    #[test]
    fn build_prompt_from_context() {
        let engine = ReasoningEngine::new(3, 0.5);
        let context = sample_context();

        let prompt = engine.build_prompt(&context);

        assert!(prompt.system.contains("Generate at most 3 intents"));
        assert_eq!(prompt.messages.len(), 1);
        assert_eq!(prompt.messages[0].role, PromptRole::User);
        assert!(prompt.messages[0]
            .content
            .contains("Goal: Draft and send a reply"));
        assert!(prompt.messages[0].content.contains("last_contact = Alex"));
        assert!(prompt.messages[0].content.contains("Identity context:"));
        assert!(prompt.messages[0].content.contains("User name: Joe"));
        assert!(prompt.messages[0].content.contains("tone: direct"));
        assert!(prompt.messages[0]
            .content
            .contains("Personality traits: helpful"));
        assert!(prompt.tools.iter().any(|tool| tool.name == "emit_intent"));
        assert!(prompt
            .tools
            .iter()
            .any(|tool| tool.name == "procedure_reply_template"));
    }

    #[test]
    fn build_prompt_deduplicates_colliding_tool_names() {
        let engine = ReasoningEngine::new(3, 0.5);
        let mut context = sample_context();
        context.active_procedures = vec![
            ProcedureRef {
                id: "reply-template".to_owned(),
                name: "Reply Template".to_owned(),
                version: 2,
            },
            ProcedureRef {
                id: "reply_template".to_owned(),
                name: "Duplicate Reply Template".to_owned(),
                version: 3,
            },
        ];

        let prompt = engine.build_prompt(&context);
        let matching_tools = prompt
            .tools
            .iter()
            .filter(|tool| tool.name == "procedure_reply_template")
            .count();

        assert_eq!(matching_tools, 1);
    }

    #[test]
    fn parse_response_filters_threshold_and_invalid_payloads() {
        let engine = ReasoningEngine::new(2, 0.6);
        let context = sample_context();

        let valid_payload = serde_json::to_string(&json!({
            "intents": [
                {
                    "action": { "Respond": { "text": "On it." } },
                    "rationale": "Immediate acknowledgement",
                    "confidence": 0.9,
                    "expected_outcome": null,
                    "sub_goals": []
                },
                {
                    "action": { "Respond": { "text": "I need more details." } },
                    "rationale": "Low certainty fallback",
                    "confidence": 0.2,
                    "expected_outcome": null,
                    "sub_goals": []
                },
                {
                    "action": { "Respond": { "text": "Sending full reply now." } },
                    "rationale": "Complete response",
                    "confidence": 0.8,
                    "expected_outcome": null,
                    "sub_goals": []
                }
            ]
        }))
        .expect("serialize test payload");

        let parsed = engine.parse_response(&valid_payload, &context);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|intent| intent.confidence >= 0.6));
        assert_eq!(parsed[0].rationale, "Immediate acknowledgement");
        assert_eq!(parsed[1].rationale, "Complete response");

        let invalid = engine.parse_response("definitely not json", &context);
        assert!(invalid.is_empty());
    }

    #[test]
    fn parse_response_supports_fenced_json_with_leading_prose() {
        let engine = ReasoningEngine::new(3, 0.0);
        let context = sample_context();

        let fenced = "Here are the intents:\n```json\n{\"intents\":[{\"action\":{\"Respond\":{\"text\":\"Done\"}},\"rationale\":\"Complete\",\"confidence\":0.7,\"expected_outcome\":null,\"sub_goals\":[]}]}\n```\nUse these.";
        let parsed = engine.parse_response(fenced, &context);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].rationale, "Complete");
    }

    #[test]
    fn parse_response_prefers_last_fenced_json_block() {
        let engine = ReasoningEngine::new(3, 0.0);
        let context = sample_context();

        let response = "```json\n{\"intents\":[{\"action\":{\"Respond\":{\"text\":\"Old\"}},\"rationale\":\"Old block\",\"confidence\":0.4,\"expected_outcome\":null,\"sub_goals\":[]}]}\n```\nUpdated plan:\n```json\n{\"intents\":[{\"action\":{\"Respond\":{\"text\":\"New\"}},\"rationale\":\"New block\",\"confidence\":0.8,\"expected_outcome\":null,\"sub_goals\":[]}]}\n```";
        let parsed = engine.parse_response(response, &context);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].rationale, "New block");
    }

    #[test]
    fn parse_response_supports_json_embedded_in_prose_without_fences() {
        let engine = ReasoningEngine::new(3, 0.0);
        let context = sample_context();

        let embedded = "Intent plan follows: {\"intents\":[{\"action\":{\"Respond\":{\"text\":\"Working on it\"}},\"rationale\":\"Acknowledge request\",\"confidence\":0.65,\"expected_outcome\":null,\"sub_goals\":[]}]} Thanks.";
        let parsed = engine.parse_response(embedded, &context);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].rationale, "Acknowledge request");
    }

    #[test]
    fn parse_response_clamps_confidence_to_unit_interval() {
        let engine = ReasoningEngine::new(3, 0.9);
        let context = sample_context();

        let payload = serde_json::to_string(&json!({
            "intents": [
                {
                    "action": { "Respond": { "text": "Confident" } },
                    "rationale": "Model over-reported confidence",
                    "confidence": 1.7,
                    "expected_outcome": null,
                    "sub_goals": []
                },
                {
                    "action": { "Respond": { "text": "Negative confidence" } },
                    "rationale": "Invalid negative confidence",
                    "confidence": -0.3,
                    "expected_outcome": null,
                    "sub_goals": []
                }
            ]
        }))
        .expect("serialize confidence payload");

        let parsed = engine.parse_response(&payload, &context);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].confidence, 1.0);
        assert_eq!(parsed[0].rationale, "Model over-reported confidence");
    }
}

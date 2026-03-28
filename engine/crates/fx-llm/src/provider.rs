//! Provider abstraction for model-completion backends.

use async_trait::async_trait;
use futures::Stream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use std::collections::HashMap;
use std::pin::Pin;

use crate::streaming::{emit_default_stream_response, StreamCallback};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, StreamChunk, ToolCall,
};

/// Streaming response type for completion APIs.
pub type CompletionStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

/// Static capabilities for a provider backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// Whether this backend accepts a `temperature` request parameter.
    pub supports_temperature: bool,
    /// Whether this backend requires streaming to be used.
    pub requires_streaming: bool,
}

/// Provider-specific catalog filtering policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProviderCatalogFilters {
    /// Apply the shared recency and price-floor filter used for OpenRouter catalogs.
    /// More provider-specific catalog gates can be added here as metadata
    /// contracts expand without proliferating ad hoc boolean methods.
    pub apply_recency_and_price_floor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopTextDeltaMode {
    Emit,
    Suppress,
}

impl LoopTextDeltaMode {
    pub const fn should_emit(self) -> bool {
        matches!(self, Self::Emit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopBufferedCompletionStrategy {
    AggregateStream,
    SingleResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStreamingRecoveryStrategy {
    Fail,
    RetryWithSingleResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopPromptOverlayContext {
    Reasoning,
    ToolContinuation,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoopResponseTextClassification {
    Text(String),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoopResponseClassification {
    UseTools {
        tool_calls: Vec<ToolCall>,
        provider_ids: HashMap<String, String>,
    },
    Respond(LoopResponseTextClassification),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopModelMatch {
    Prefix(&'static str),
    Contains(&'static str),
    AnyPrefix(&'static [&'static str]),
    AnyContains(&'static [&'static str]),
    Any,
}

impl LoopModelMatch {
    fn matches(self, model: &str) -> bool {
        let normalized = normalized_model_name(model);
        match self {
            Self::Prefix(prefix) => normalized.starts_with(prefix),
            Self::Contains(fragment) => normalized.contains(fragment),
            Self::AnyPrefix(prefixes) => {
                prefixes.iter().any(|prefix| normalized.starts_with(prefix))
            }
            Self::AnyContains(fragments) => fragments
                .iter()
                .any(|fragment| normalized.contains(fragment)),
            Self::Any => true,
        }
    }
}

pub trait LoopHarness: Send + Sync + std::fmt::Debug {
    fn reason_text_mode(&self, has_callback: bool) -> LoopTextDeltaMode {
        if has_callback {
            LoopTextDeltaMode::Suppress
        } else {
            LoopTextDeltaMode::Emit
        }
    }

    fn buffered_completion_strategy(&self) -> LoopBufferedCompletionStrategy {
        LoopBufferedCompletionStrategy::AggregateStream
    }

    fn prompt_overlay(&self, _context: LoopPromptOverlayContext) -> Option<&'static str> {
        None
    }

    fn build_truncation_resume_messages(
        &self,
        base_messages: &[Message],
        full_text: &str,
    ) -> Vec<Message> {
        default_loop_truncation_resume_messages(base_messages, full_text)
    }

    fn classify_response(&self, response: &CompletionResponse) -> LoopResponseClassification {
        default_loop_response_classification(response)
    }

    fn is_truncated(&self, stop_reason: Option<&str>) -> bool {
        matches!(
            normalized_stop_reason(stop_reason).as_deref(),
            Some("length" | "max_tokens" | "incomplete")
        )
    }

    fn streaming_recovery(
        &self,
        _error: &LlmError,
        _emitted_text: bool,
    ) -> LoopStreamingRecoveryStrategy {
        LoopStreamingRecoveryStrategy::Fail
    }
}

#[derive(Debug)]
struct NullLoopHarness;

impl LoopHarness for NullLoopHarness {}

pub trait LoopModelProfile: Send + Sync + std::fmt::Debug {
    fn label(&self) -> &'static str;
    fn matches_model(&self, model: &str) -> bool;
    fn harness(&self) -> &'static dyn LoopHarness;
}

#[derive(Debug)]
pub struct StaticLoopModelProfile {
    pub label: &'static str,
    pub matcher: LoopModelMatch,
    pub harness: &'static dyn LoopHarness,
}

impl LoopModelProfile for StaticLoopModelProfile {
    fn label(&self) -> &'static str {
        self.label
    }

    fn matches_model(&self, model: &str) -> bool {
        self.matcher.matches(model)
    }

    fn harness(&self) -> &'static dyn LoopHarness {
        self.harness
    }
}

fn normalized_stop_reason(stop_reason: Option<&str>) -> Option<String> {
    stop_reason.map(|reason| reason.trim().to_ascii_lowercase())
}

pub fn normalized_model_name(model: &str) -> &str {
    model.split('/').next_back().unwrap_or(model)
}

fn response_text_blocks(response: &CompletionResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::Image { .. }
            | ContentBlock::Document { .. }
            | ContentBlock::ToolUse { .. }
            | ContentBlock::ToolResult { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn readable_response_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return raw.to_string();
    }

    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed) {
        for key in &["text", "response", "message", "content", "answer"] {
            if let Some(val) = obj.get(key).and_then(|value| value.as_str()) {
                return val.to_string();
            }
        }
    }

    raw.to_string()
}

fn response_provider_ids(content: &[ContentBlock]) -> HashMap<String, String> {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse {
                id,
                provider_id: Some(provider_id),
                ..
            } if !id.trim().is_empty() && !provider_id.trim().is_empty() => {
                Some((id.clone(), provider_id.clone()))
            }
            _ => None,
        })
        .collect()
}

pub fn default_loop_truncation_resume_messages(
    base_messages: &[Message],
    full_text: &str,
) -> Vec<Message> {
    let mut continuation_messages = base_messages.to_vec();
    if !full_text.trim().is_empty() {
        continuation_messages.push(Message::assistant(full_text.to_string()));
    }
    continuation_messages.push(Message::user(
        "Continue from exactly where you left off. Do not repeat prior text.",
    ));
    continuation_messages
}

pub fn default_loop_response_classification(
    response: &CompletionResponse,
) -> LoopResponseClassification {
    if !response.tool_calls.is_empty() {
        return LoopResponseClassification::UseTools {
            tool_calls: response.tool_calls.clone(),
            provider_ids: response_provider_ids(&response.content),
        };
    }

    let raw = response_text_blocks(response);
    let text = readable_response_text(&raw);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        LoopResponseClassification::Respond(LoopResponseTextClassification::Empty)
    } else {
        LoopResponseClassification::Respond(LoopResponseTextClassification::Text(
            trimmed.to_string(),
        ))
    }
}

static NULL_LOOP_HARNESS: NullLoopHarness = NullLoopHarness;

pub fn null_loop_harness() -> &'static dyn LoopHarness {
    &NULL_LOOP_HARNESS
}

pub fn resolve_loop_harness_from_profiles(
    profiles: &[&'static dyn LoopModelProfile],
    model: &str,
    fallback: &'static dyn LoopHarness,
) -> &'static dyn LoopHarness {
    profiles
        .iter()
        .find(|profile| profile.matches_model(model))
        .map(|profile| profile.harness())
        .unwrap_or(fallback)
}

fn authorization_header_value(api_key: &str) -> Result<HeaderValue, String> {
    let bearer = format!("Bearer {api_key}");
    HeaderValue::from_str(&bearer).map_err(|error| format!("invalid authorization header: {error}"))
}

pub(crate) fn insert_bearer_authorization(
    headers: &mut HeaderMap,
    api_key: &str,
) -> Result<(), String> {
    let value = authorization_header_value(api_key)?;
    headers.insert(AUTHORIZATION, value);
    Ok(())
}

pub(crate) fn insert_header_value(
    headers: &mut HeaderMap,
    name: &'static str,
    value: &str,
    label: &str,
) -> Result<(), String> {
    let header =
        HeaderValue::from_str(value).map_err(|error| format!("invalid {label} header: {error}"))?;
    headers.insert(name, header);
    Ok(())
}

pub(crate) fn bearer_auth_headers(api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    insert_bearer_authorization(&mut headers, api_key)?;
    Ok(headers)
}

/// Shared provider interface for cloud LLM adapters.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a completion request and return the full response.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;

    /// Send a completion request and return a stream of incremental chunks.
    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, LlmError>;

    /// Send a completion request and emit normalized stream events.
    async fn stream(
        &self,
        request: CompletionRequest,
        callback: StreamCallback,
    ) -> Result<CompletionResponse, LlmError> {
        let response = self.complete(request).await?;
        emit_default_stream_response(&response, &callback);
        Ok(response)
    }

    /// Provider name for logging/routing.
    fn name(&self) -> &str;

    /// Models supported by this provider.
    fn supported_models(&self) -> Vec<String>;

    /// Fetch available models dynamically from the provider API.
    ///
    /// Returns model IDs the current credential has access to. Providers
    /// without a dynamic catalog override fall back to their static support list.
    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(self.supported_models())
    }

    /// Provider feature support contract.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Thinking-effort levels accepted by this provider.
    fn supported_thinking_levels(&self) -> &'static [&'static str] {
        &["off"]
    }

    /// User-facing thinking levels accepted for a specific model.
    fn thinking_levels(&self, _model: &str) -> &'static [&'static str] {
        self.supported_thinking_levels()
    }

    /// Optional models endpoint used for catalog fetches.
    fn models_endpoint(&self) -> Option<&str> {
        None
    }

    /// Primary auth method label for models served by this provider instance.
    fn auth_method(&self) -> &'static str {
        "api_key"
    }

    /// Authentication headers for model catalog requests.
    fn catalog_auth_headers(&self, api_key: &str, _auth_mode: &str) -> Result<HeaderMap, String> {
        bearer_auth_headers(api_key)
    }

    /// Provider-specific chat-model filter for catalog payloads.
    fn is_chat_capable(&self, _model_id: &str) -> bool {
        true
    }

    /// Provider-specific static catalog fallback.
    fn fallback_models(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Provider-specific catalog filtering knobs.
    fn catalog_filters(&self) -> ProviderCatalogFilters {
        ProviderCatalogFilters::default()
    }

    /// Provider-owned context window lookup for a specific model.
    fn context_window(&self, _model: &str) -> usize {
        128_000
    }

    /// Provider-owned loop harness semantics for the given model.
    fn loop_harness(&self, _model: &str) -> &'static dyn LoopHarness {
        null_loop_harness()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct DummyHarness;

    impl LoopHarness for DummyHarness {}

    static MATCHING_HARNESS: DummyHarness = DummyHarness;
    static FALLBACK_HARNESS: DummyHarness = DummyHarness;

    static PREFIX_PROFILE: StaticLoopModelProfile = StaticLoopModelProfile {
        label: "prefix",
        matcher: LoopModelMatch::AnyPrefix(&["gpt-5.4", "codex-"]),
        harness: &MATCHING_HARNESS,
    };

    static DEFAULT_PROFILE: StaticLoopModelProfile = StaticLoopModelProfile {
        label: "default",
        matcher: LoopModelMatch::Any,
        harness: &FALLBACK_HARNESS,
    };

    #[test]
    fn resolve_loop_harness_from_profiles_uses_first_matching_profile() {
        let profiles: [&'static dyn LoopModelProfile; 2] = [&PREFIX_PROFILE, &DEFAULT_PROFILE];
        let resolved =
            resolve_loop_harness_from_profiles(&profiles, "openai/gpt-5.4", null_loop_harness());

        assert!(std::ptr::eq(
            resolved as *const dyn LoopHarness,
            &MATCHING_HARNESS as &dyn LoopHarness as *const dyn LoopHarness,
        ));
    }

    #[test]
    fn resolve_loop_harness_from_profiles_falls_back_when_no_profile_matches() {
        let profiles: [&'static dyn LoopModelProfile; 1] = [&PREFIX_PROFILE];
        let resolved =
            resolve_loop_harness_from_profiles(&profiles, "claude-opus-4-6", &FALLBACK_HARNESS);

        assert!(std::ptr::eq(
            resolved as *const dyn LoopHarness,
            &FALLBACK_HARNESS as &dyn LoopHarness as *const dyn LoopHarness,
        ));
    }
}

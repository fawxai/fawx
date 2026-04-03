use async_trait::async_trait;
use futures::StreamExt;
use fx_config::ThinkingBudget;
use fx_core::error::LlmError as CoreLlmError;
use fx_kernel::loop_engine::{LlmProvider as LoopLlmProvider, LoopStatus};
use fx_llm::{
    null_loop_harness, CompletionRequest, LoopHarness, Message, ModelInfo, ModelRouter,
    ProviderError, StreamCallback, StreamChunk, ThinkingConfig,
};
use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, RwLock};

fn highest_version_model(model_ids: &[String]) -> Option<&str> {
    model_ids
        .iter()
        .filter_map(|id| {
            let parts = version_parts(id);
            if parts.is_empty() {
                None
            } else {
                Some((id, parts))
            }
        })
        .max_by(|(_, left), (_, right)| left.cmp(right))
        .map(|(id, _)| id.as_str())
}

fn version_parts(model_id: &str) -> Vec<u32> {
    model_id
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

pub(crate) fn resolve_model_alias(selector: &str, model_ids: &[String]) -> Option<String> {
    let family_prefix = claude_family_prefix(selector)?;
    let matches = model_ids
        .iter()
        .filter(|model_id| model_id.starts_with(&family_prefix))
        .cloned()
        .collect::<Vec<_>>();

    highest_version_model(&matches).map(ToString::to_string)
}

fn claude_family_prefix(selector: &str) -> Option<String> {
    let mut parts = selector.split('-');
    let provider = parts.next()?;
    let family = parts.next()?.to_ascii_lowercase();
    let major = parts.next()?;

    if !provider.eq_ignore_ascii_case("claude") {
        return None;
    }
    if family.is_empty() || !family.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    if major.parse::<u32>().is_err() {
        return None;
    }

    Some(format!("claude-{family}-{major}-"))
}

fn group_models_by_provider(models: &[ModelInfo]) -> Vec<(String, Vec<&ModelInfo>)> {
    let mut groups: Vec<(String, Vec<&ModelInfo>)> = Vec::new();
    for model in models {
        if let Some(existing) = groups
            .iter_mut()
            .find(|(name, _)| *name == model.provider_name)
        {
            existing.1.push(model);
        } else {
            groups.push((model.provider_name.clone(), vec![model]));
        }
    }
    groups
}

pub(crate) fn render_model_menu_text(active: Option<&str>, models: &[ModelInfo]) -> String {
    if models.is_empty() {
        return "No models available. Use /auth to configure credentials.".to_string();
    }

    let grouped = group_models_by_provider(models);
    let mut lines = vec!["Available models:".to_string()];
    for (provider, group) in grouped {
        lines.push(String::new());
        lines.push(format!("{provider}:"));
        for model in group {
            let marker = if active == Some(model.model_id.as_str()) {
                "*"
            } else {
                " "
            };
            lines.push(format!(
                "  {marker} {} ({})",
                model.model_id, model.auth_method
            ));
        }
    }
    lines.join("\n")
}

pub(crate) fn render_status_text(model: &str, providers: &[String], status: LoopStatus) -> String {
    [
        "Fawx Status".to_string(),
        format!("  model:     {model}"),
        format!("  providers: {}", providers.join(", ")),
        format!("  tokens:    {} used", status.tokens_used),
        format!("  budget:    {} tokens remaining", status.remaining.tokens),
    ]
    .join("\n")
}

pub(crate) fn available_provider_names(router: &ModelRouter) -> Vec<String> {
    let mut providers = Vec::new();
    for model in router.available_models() {
        if !providers.contains(&model.provider_name) {
            providers.push(model.provider_name);
        }
    }
    providers
}

pub(crate) type SharedModelRouter = Arc<RwLock<ModelRouter>>;

pub(crate) fn read_router<T>(router: &SharedModelRouter, op: impl FnOnce(&ModelRouter) -> T) -> T {
    match router.read() {
        Ok(guard) => op(&guard),
        Err(error) => {
            tracing::warn!(error = %error, "router lock poisoned");
            let guard = error.into_inner();
            op(&guard)
        }
    }
}

pub(crate) fn write_router<T>(
    router: &SharedModelRouter,
    op: impl FnOnce(&mut ModelRouter) -> T,
) -> T {
    match router.write() {
        Ok(mut guard) => op(&mut guard),
        Err(error) => {
            tracing::warn!(error = %error, "router lock poisoned");
            let mut guard = error.into_inner();
            op(&mut guard)
        }
    }
}

pub(crate) async fn fetch_shared_available_models(router: &SharedModelRouter) -> Vec<ModelInfo> {
    let catalog = read_router(router, ModelRouter::provider_catalog);
    fx_llm::fetch_available_models_from_catalog(catalog).await
}

fn prepare_router_request(
    router: &SharedModelRouter,
    active_model: &str,
    request: CompletionRequest,
) -> Result<(Arc<dyn fx_llm::CompletionProvider>, CompletionRequest), ProviderError> {
    read_router(router, |router| {
        router.request_for_model(active_model, request)
    })
}

fn resolve_loop_harness(
    router: &SharedModelRouter,
    active_model: &str,
) -> &'static dyn LoopHarness {
    let probe = CompletionRequest {
        model: active_model.to_string(),
        messages: Vec::new(),
        tools: Vec::new(),
        temperature: None,
        max_tokens: None,
        system_prompt: None,
        thinking: None,
    };
    read_router(router, |router| {
        let Ok((provider, _)) = router.request_for_model(active_model, probe) else {
            return null_loop_harness();
        };
        provider.loop_harness(active_model)
    })
}

/// Convert a thinking budget level into a provider-specific [`ThinkingConfig`].
///
/// Uses the active model ID to determine the correct wire format:
/// - Anthropic 4.6: `Adaptive { effort }`
/// - Anthropic 4.5: `Enabled { budget_tokens }`
/// - OpenAI: `Reasoning { effort }`
pub(crate) fn thinking_config_for_active_model(
    budget: &ThinkingBudget,
    model_id: &str,
) -> Option<ThinkingConfig> {
    let level = budget.to_string();
    if level == "off" {
        return None;
    }
    fx_llm::thinking_config_for_model(model_id, &level)
}

pub(crate) fn format_memory_for_prompt(
    entries: &[(String, String)],
    max_chars: usize,
) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut text = String::from("What you remember from previous sessions:\n");
    for (key, value) in entries {
        let line = format!("- {key}: {value}\n");
        if text.len() + line.len() > max_chars {
            text.push_str("(truncated)\n");
            break;
        }
        text.push_str(&line);
    }
    text.push_str(
        "(Use memory_read for details. \
        Use memory_write to update or add memories.)",
    );
    Some(text)
}

/// Thin wrapper to expose `ModelRouter` as a `CompletionProvider` for analysis.
pub(crate) struct AnalysisCompletionProvider {
    router: SharedModelRouter,
    active_model: String,
}

impl fmt::Debug for AnalysisCompletionProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisCompletionProvider")
            .field("active_model", &self.active_model)
            .finish()
    }
}

impl AnalysisCompletionProvider {
    pub(crate) fn new(router: SharedModelRouter, active_model: String) -> Self {
        Self {
            router,
            active_model,
        }
    }

    fn route_request(
        &self,
        request: CompletionRequest,
    ) -> Result<(Arc<dyn fx_llm::CompletionProvider>, CompletionRequest), ProviderError> {
        prepare_router_request(&self.router, &self.active_model, request)
    }
}

#[async_trait]
impl fx_llm::CompletionProvider for AnalysisCompletionProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
        let (provider, request) = self.route_request(request)?;
        provider.complete(request).await
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
        let (provider, request) = self.route_request(request)?;
        provider.complete_stream(request).await
    }

    fn name(&self) -> &str {
        "analysis"
    }

    fn supported_models(&self) -> Vec<String> {
        vec![self.active_model.clone()]
    }

    fn capabilities(&self) -> fx_llm::ProviderCapabilities {
        fx_llm::ProviderCapabilities {
            supports_temperature: true,
            requires_streaming: false,
        }
    }
}

pub(crate) struct RouterLoopLlmProvider {
    router: SharedModelRouter,
    active_model: String,
}

impl fmt::Debug for RouterLoopLlmProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouterLoopLlmProvider")
            .field("active_model", &self.active_model)
            .finish()
    }
}

impl RouterLoopLlmProvider {
    pub(crate) fn new(router: SharedModelRouter, active_model: String) -> Self {
        let _ = resolve_loop_harness(&router, &active_model);
        Self {
            router,
            active_model,
        }
    }

    fn route_request(
        &self,
        request: CompletionRequest,
    ) -> Result<(Arc<dyn fx_llm::CompletionProvider>, CompletionRequest), ProviderError> {
        prepare_router_request(&self.router, &self.active_model, request)
    }
}

#[async_trait]
impl LoopLlmProvider for RouterLoopLlmProvider {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, CoreLlmError> {
        let request = CompletionRequest {
            model: self.active_model.clone(),
            messages: vec![Message::user(prompt)],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(max_tokens),
            system_prompt: Some(prompt.to_string()),
            thinking: None,
        };

        let (provider, request) = self
            .route_request(request)
            .map_err(|error| CoreLlmError::Inference(error.to_string()))?;
        let mut stream = provider
            .complete_stream(request)
            .await
            .map_err(|error| CoreLlmError::Inference(error.to_string()))?;

        let collected = consume_stream_silent(&mut stream).await?;

        if collected.trim().is_empty() {
            Err(CoreLlmError::InvalidResponse(
                "provider returned an empty completion".to_string(),
            ))
        } else {
            Ok(collected)
        }
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        let request = CompletionRequest {
            model: self.active_model.clone(),
            messages: vec![Message::user(prompt)],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(max_tokens),
            system_prompt: Some(prompt.to_string()),
            thinking: None,
        };

        let (provider, request) = self
            .route_request(request)
            .map_err(|error| CoreLlmError::Inference(error.to_string()))?;
        let mut stream = provider
            .complete_stream(request)
            .await
            .map_err(|error| CoreLlmError::Inference(error.to_string()))?;
        let collected = consume_stream_with_callback(&mut stream, callback).await?;

        if collected.trim().is_empty() {
            return Err(CoreLlmError::InvalidResponse(
                "provider returned an empty completion".to_string(),
            ));
        }

        Ok(collected)
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionResponse, ProviderError> {
        let (provider, request) = self.route_request(request)?;
        provider.complete(request).await
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionStream, ProviderError> {
        let (provider, request) = self.route_request(request)?;
        provider.complete_stream(request).await
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        callback: StreamCallback,
    ) -> Result<fx_llm::CompletionResponse, ProviderError> {
        let (provider, request) = self.route_request(request)?;
        provider.stream(request, callback).await
    }

    fn model_name(&self) -> &str {
        &self.active_model
    }
}

/// Collect all stream chunks into a string without printing.
async fn consume_stream_silent(
    stream: &mut (impl futures::Stream<Item = Result<StreamChunk, ProviderError>> + Unpin),
) -> Result<String, CoreLlmError> {
    let mut sink = io::sink();
    consume_stream_with_writer(stream, &mut sink).await
}

async fn consume_stream_with_callback(
    stream: &mut (impl futures::Stream<Item = Result<StreamChunk, ProviderError>> + Unpin),
    callback: Box<dyn Fn(String) + Send + 'static>,
) -> Result<String, CoreLlmError> {
    let mut collected = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                if let Some(delta) = chunk.delta_content {
                    collected.push_str(&delta);
                    callback(delta);
                }
            }
            Err(error) => return Err(CoreLlmError::Inference(error.to_string())),
        }
    }
    Ok(collected)
}

async fn consume_stream_with_writer(
    stream: &mut (impl futures::Stream<Item = Result<StreamChunk, ProviderError>> + Unpin),
    writer: &mut impl Write,
) -> Result<String, CoreLlmError> {
    let mut collected = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                if let Some(delta) = &chunk.delta_content {
                    write_stream_delta(writer, delta)?;
                    collected.push_str(delta);
                }
            }
            Err(error) => return Err(CoreLlmError::Inference(error.to_string())),
        }
    }
    Ok(collected)
}

fn write_stream_delta(writer: &mut impl Write, delta: &str) -> Result<(), CoreLlmError> {
    writer
        .write_all(delta.as_bytes())
        .map_err(|error| CoreLlmError::Inference(error.to_string()))?;
    writer
        .flush()
        .map_err(|error| CoreLlmError::Inference(error.to_string()))
}

pub(crate) fn trim_history(history: &mut Vec<Message>, max_history: usize) {
    fx_llm::trim_conversation_history(history, max_history);
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use fx_config::ThinkingBudget;
    use fx_llm::{
        CompletionProvider, CompletionResponse, CompletionStream, LoopBufferedCompletionStrategy,
        LoopPromptOverlayContext, ProviderCapabilities,
    };
    use std::sync::{Arc, Mutex, RwLock};

    fn shared_router(router: ModelRouter) -> SharedModelRouter {
        Arc::new(RwLock::new(router))
    }

    #[derive(Debug)]
    struct ModelEchoProvider {
        provider_name: String,
        models: Vec<String>,
    }

    #[derive(Debug)]
    struct ResponsesHarness;

    impl LoopHarness for ResponsesHarness {
        fn buffered_completion_strategy(&self) -> LoopBufferedCompletionStrategy {
            LoopBufferedCompletionStrategy::SingleResponse
        }
    }

    #[derive(Debug)]
    struct ClaudeHarness;

    impl LoopHarness for ClaudeHarness {
        fn prompt_overlay(&self, context: LoopPromptOverlayContext) -> Option<&'static str> {
            match context {
                LoopPromptOverlayContext::Reasoning => {
                    Some("\n\nModel-family guidance for Claude models")
                }
                LoopPromptOverlayContext::ToolContinuation => None,
            }
        }
    }

    static RESPONSES_HARNESS: ResponsesHarness = ResponsesHarness;
    static CLAUDE_HARNESS: ClaudeHarness = ClaudeHarness;

    #[async_trait]
    impl CompletionProvider for ModelEchoProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            unimplemented!()
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            unimplemented!()
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            self.models.clone()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }

        fn loop_harness(&self, model: &str) -> &'static dyn LoopHarness {
            if model.starts_with("claude-") {
                &CLAUDE_HARNESS
            } else {
                null_loop_harness()
            }
        }
    }

    #[derive(Debug)]
    struct StreamingProvider {
        provider_name: String,
        model: String,
        chunks: Vec<&'static str>,
    }

    #[async_trait]
    impl CompletionProvider for StreamingProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            unimplemented!()
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            let chunks = self
                .chunks
                .iter()
                .map(|chunk| {
                    Ok(StreamChunk {
                        delta_content: Some((*chunk).to_string()),
                        tool_use_deltas: Vec::new(),
                        usage: None,
                        stop_reason: None,
                    })
                })
                .collect::<Vec<_>>();
            Ok(Box::pin(stream::iter(chunks)))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: true,
            }
        }

        fn loop_harness(&self, _model: &str) -> &'static dyn LoopHarness {
            &RESPONSES_HARNESS
        }
    }

    #[test]
    fn resolve_model_alias_supports_new_claude_families() {
        let models = vec![
            "claude-haiku-4-20240101".to_string(),
            "claude-haiku-4-20250514".to_string(),
        ];

        let resolved = resolve_model_alias("claude-haiku-4-6", &models);

        assert_eq!(resolved.as_deref(), Some("claude-haiku-4-20250514"));
    }

    #[test]
    fn group_models_by_provider_groups_correctly() {
        let models = vec![
            ModelInfo {
                model_id: "claude-sonnet-4-20260514".to_string(),
                provider_name: "Anthropic".to_string(),
                auth_method: "subscription".to_string(),
            },
            ModelInfo {
                model_id: "claude-opus-4-20260514".to_string(),
                provider_name: "Anthropic".to_string(),
                auth_method: "subscription".to_string(),
            },
            ModelInfo {
                model_id: "gpt-4o".to_string(),
                provider_name: "OpenAI".to_string(),
                auth_method: "api_key".to_string(),
            },
        ];

        let grouped = group_models_by_provider(&models);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "Anthropic");
        assert_eq!(grouped[0].1.len(), 2);
        assert_eq!(grouped[1].0, "OpenAI");
        assert_eq!(grouped[1].1.len(), 1);
    }

    #[test]
    fn available_provider_names_deduplicate_available_models() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ModelEchoProvider {
            provider_name: "Anthropic".to_string(),
            models: vec![
                "claude-opus-4-6".to_string(),
                "claude-sonnet-4-6".to_string(),
            ],
        }));
        router.register_provider(Box::new(ModelEchoProvider {
            provider_name: "OpenAI".to_string(),
            models: vec!["gpt-4o".to_string()],
        }));

        assert_eq!(
            available_provider_names(&router),
            vec!["Anthropic".to_string(), "OpenAI".to_string()]
        );
    }

    #[test]
    fn thinking_config_for_active_model_maps_claude_4_6_to_adaptive() {
        let thinking =
            thinking_config_for_active_model(&ThinkingBudget::Low, "claude-opus-4-6-20250929");

        assert_eq!(
            thinking,
            Some(ThinkingConfig::Adaptive {
                effort: "low".to_string(),
            })
        );
    }

    #[test]
    fn thinking_config_for_active_model_maps_claude_4_5_to_enabled_budget() {
        let thinking =
            thinking_config_for_active_model(&ThinkingBudget::High, "claude-sonnet-4-5-20250929");

        assert_eq!(
            thinking,
            Some(ThinkingConfig::Enabled {
                budget_tokens: 10_000,
            })
        );
    }

    #[test]
    fn thinking_config_for_active_model_maps_openai_to_reasoning() {
        let thinking = thinking_config_for_active_model(&ThinkingBudget::High, "gpt-5.4");

        assert_eq!(
            thinking,
            Some(ThinkingConfig::Reasoning {
                effort: "high".to_string(),
            })
        );
    }

    #[test]
    fn thinking_config_for_active_model_returns_none_when_disabled() {
        let thinking = thinking_config_for_active_model(&ThinkingBudget::Off, "claude-opus-4-6");

        assert_eq!(thinking, None);
    }

    #[test]
    fn format_memory_for_prompt_truncates_entries() {
        let entries = vec![
            ("project".to_string(), "remove legacy tui".to_string()),
            ("long".to_string(), "x".repeat(80)),
        ];

        let rendered = format_memory_for_prompt(&entries, 90).expect("memory prompt");

        assert!(rendered.contains("project: remove legacy tui"));
        assert!(rendered.contains("(truncated)"));
    }

    #[test]
    fn write_router_updates_shared_router() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ModelEchoProvider {
            provider_name: "OpenAI".to_string(),
            models: vec!["gpt-5.4".to_string()],
        }));
        let router = shared_router(router);

        write_router(&router, |router| {
            router.set_active("gpt-5.4").expect("set active");
        });

        let active_model = read_router(&router, |router| {
            router.active_model().map(ToString::to_string)
        });
        assert_eq!(active_model.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn resolve_loop_harness_uses_provider_owned_responses_semantics() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(StreamingProvider {
            provider_name: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            chunks: vec!["hello"],
        }));
        let router = shared_router(router);

        assert_eq!(
            resolve_loop_harness(&router, "gpt-5.4").buffered_completion_strategy(),
            LoopBufferedCompletionStrategy::SingleResponse
        );
    }

    #[test]
    fn resolve_loop_harness_uses_provider_owned_prompt_overlay() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ModelEchoProvider {
            provider_name: "anthropic".to_string(),
            models: vec!["claude-opus-4-6".to_string()],
        }));
        let router = shared_router(router);

        assert_eq!(
            resolve_loop_harness(&router, "claude-opus-4-6")
                .prompt_overlay(LoopPromptOverlayContext::Reasoning),
            Some("\n\nModel-family guidance for Claude models")
        );
    }

    #[tokio::test]
    async fn fetch_shared_available_models_reads_from_shared_router() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ModelEchoProvider {
            provider_name: "Anthropic".to_string(),
            models: vec!["claude-opus-4-6".to_string()],
        }));
        let router = shared_router(router);

        let models = fetch_shared_available_models(&router).await;

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "claude-opus-4-6");
        assert_eq!(models[0].provider_name, "Anthropic");
    }

    #[test]
    fn trim_history_drops_oldest_messages() {
        let mut history = vec![
            Message::user("q1"),
            Message::assistant("a1"),
            Message::user("q2"),
            Message::assistant("a2"),
        ];

        trim_history(&mut history, 2);

        assert_eq!(history, vec![Message::user("q2"), Message::assistant("a2")]);
    }

    #[tokio::test]
    async fn router_loop_llm_provider_streams_deltas_through_callback() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(StreamingProvider {
            provider_name: "Anthropic".to_string(),
            model: "claude-opus-4-6".to_string(),
            chunks: vec!["hello", " world"],
        }));
        router.set_active("claude-opus-4-6").expect("active model");

        let router = shared_router(router);
        let provider =
            RouterLoopLlmProvider::new(Arc::clone(&router), "claude-opus-4-6".to_string());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let callback_seen = Arc::clone(&seen);

        let result = provider
            .generate_streaming(
                "test prompt",
                32,
                Box::new(move |chunk| {
                    callback_seen.lock().expect("callback chunks").push(chunk);
                }),
            )
            .await
            .expect("streamed response");

        assert_eq!(result, "hello world");
        assert_eq!(
            seen.lock().expect("callback chunks").clone(),
            vec!["hello".to_string(), " world".to_string()]
        );
    }
}

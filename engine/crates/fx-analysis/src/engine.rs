use crate::AnalysisFinding;
use fx_core::signals::{Signal, SignalKind};
use fx_llm::{
    CompletionProvider, CompletionRequest, Message, ProviderError, ToolCall, ToolDefinition,
};
use fx_memory::signal_store::SignalStoreError;
use fx_memory::SignalStore;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

const REPORT_FINDINGS_TOOL_NAME: &str = "report_findings";
const ANALYSIS_SYSTEM_PROMPT: &str = "You analyze runtime signals across sessions. Identify recurring patterns and report each one by calling the report_findings tool.";

type SessionSignal = (String, Signal);

#[derive(Debug)]
pub enum AnalysisError {
    SignalStore(SignalStoreError),
    Llm(ProviderError),
    ParseError(serde_json::Error),
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignalStore(error) => write!(f, "signal store error: {error}"),
            Self::Llm(error) => write!(f, "llm error: {error}"),
            Self::ParseError(error) => write!(f, "analysis parse error: {error}"),
        }
    }
}

impl std::error::Error for AnalysisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SignalStore(error) => Some(error),
            Self::Llm(error) => Some(error),
            Self::ParseError(error) => Some(error),
        }
    }
}

impl From<SignalStoreError> for AnalysisError {
    fn from(value: SignalStoreError) -> Self {
        Self::SignalStore(value)
    }
}

impl From<ProviderError> for AnalysisError {
    fn from(value: ProviderError) -> Self {
        Self::Llm(value)
    }
}

impl From<serde_json::Error> for AnalysisError {
    fn from(value: serde_json::Error) -> Self {
        Self::ParseError(value)
    }
}

#[must_use]
pub fn report_findings_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: REPORT_FINDINGS_TOOL_NAME.to_string(),
        description: "Report recurring analysis findings from runtime signals.".to_string(),
        parameters: report_findings_parameters(),
    }
}

fn report_findings_parameters() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["findings"],
        "properties": {
            "findings": {
                "type": "array",
                "items": finding_schema()
            }
        }
    })
}

fn finding_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["pattern_name", "description", "confidence", "evidence"],
        "properties": {
            "pattern_name": { "type": "string" },
            "description": { "type": "string" },
            "confidence": {
                "type": "string",
                "enum": ["high", "medium", "low"]
            },
            "evidence": {
                "type": "array",
                "items": evidence_schema()
            },
            "suggested_action": { "type": "string" }
        }
    })
}

fn evidence_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["session_id", "signal_kind", "message", "timestamp_ms"],
        "properties": {
            "session_id": { "type": "string" },
            "signal_kind": {
                "type": "string",
                "enum": signal_kind_values()
            },
            "message": { "type": "string" },
            "timestamp_ms": { "type": "integer", "minimum": 0 }
        }
    })
}

/// Signal kind string values for the tool definition schema.
///
/// Uses `SignalKind::to_label()` which produces snake_case strings matching
/// the `#[serde(rename_all = "snake_case")]` serialization format.
/// If a new `SignalKind` variant is added, add it here too.
fn signal_kind_values() -> Vec<&'static str> {
    // Exhaustive match — compiler error if a SignalKind variant is added without updating.
    fn _assert_exhaustive(k: SignalKind) {
        match k {
            SignalKind::Trace
            | SignalKind::Thinking
            | SignalKind::Friction
            | SignalKind::Success
            | SignalKind::Blocked
            | SignalKind::Performance
            | SignalKind::UserIntervention
            | SignalKind::UserInput
            | SignalKind::UserFeedback
            | SignalKind::Decision
            | SignalKind::Observation => {}
        }
    }

    // NOTE: variants intentionally listed in both the match guard above and the
    // array below — the match catches new variants at compile time, the array
    // defines the runtime list.
    [
        SignalKind::Trace,
        SignalKind::Thinking,
        SignalKind::Friction,
        SignalKind::Success,
        SignalKind::Blocked,
        SignalKind::Performance,
        SignalKind::UserIntervention,
        SignalKind::UserInput,
        SignalKind::UserFeedback,
        SignalKind::Decision,
        SignalKind::Observation,
    ]
    .iter()
    .map(|k| k.to_label())
    .collect()
}

#[derive(Debug)]
pub struct AnalysisEngine<'a> {
    signal_store: &'a SignalStore,
}

impl<'a> AnalysisEngine<'a> {
    pub fn new(signal_store: &'a SignalStore) -> Self {
        Self { signal_store }
    }

    /// Analyze stored runtime signals and return recurring findings.
    ///
    /// `CompletionRequest.model` is intentionally empty because the caller's
    /// `CompletionProvider` wrapper injects the active model before dispatch.
    pub async fn analyze(
        &self,
        provider: &dyn CompletionProvider,
    ) -> Result<Vec<AnalysisFinding>, AnalysisError> {
        let signals = self.signal_store.load_all()?;
        if signals.is_empty() {
            return Ok(Vec::new());
        }

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![Message::user(build_signal_summary(&signals))],
            tools: vec![report_findings_tool_definition()],
            temperature: None,
            max_tokens: Some(4096),
            system_prompt: Some(ANALYSIS_SYSTEM_PROMPT.to_string()),
            thinking: None,
        };

        let response = provider.complete(request).await?;
        parse_tool_call_findings(&response.tool_calls)
    }
}

fn build_signal_summary(signals: &[SessionSignal]) -> String {
    let counts = render_signal_counts(signals);
    let friction = render_recent_signals(signals, SignalKind::Friction, 8);
    let blocked = render_recent_signals(signals, SignalKind::Blocked, 8);
    let success = render_recent_signals(signals, SignalKind::Success, 8);

    format!(
        "Signal counts by kind and session:\n{counts}\n\nRecent friction signals:\n{friction}\n\nRecent blocked signals:\n{blocked}\n\nRecent success signals:\n{success}"
    )
}

fn parse_tool_call_findings(
    tool_calls: &[ToolCall],
) -> Result<Vec<AnalysisFinding>, AnalysisError> {
    let mut findings: Vec<AnalysisFinding> = tool_calls
        .iter()
        .filter(|call| call.name == REPORT_FINDINGS_TOOL_NAME)
        .try_fold(Vec::new(), collect_tool_call_findings)?;

    deduplicate_findings(&mut findings);
    Ok(findings)
}

fn deduplicate_findings(findings: &mut Vec<AnalysisFinding>) {
    let mut seen = std::collections::HashSet::new();
    findings.retain(|finding| seen.insert(finding.pattern_name.clone()));
}

fn collect_tool_call_findings(
    mut findings: Vec<AnalysisFinding>,
    tool_call: &ToolCall,
) -> Result<Vec<AnalysisFinding>, AnalysisError> {
    let args: ReportFindingsArgs = deserialize_tool_args(&tool_call.arguments)?;
    findings.extend(args.findings);
    Ok(findings)
}

/// Deserialize tool call arguments, handling both normal JSON objects
/// and double-encoded JSON strings returned by some providers.
fn deserialize_tool_args<T>(args: &serde_json::Value) -> Result<T, serde_json::Error>
where
    T: serde::de::DeserializeOwned,
{
    let original_error = match serde_json::from_value::<T>(args.clone()) {
        Ok(parsed) => return Ok(parsed),
        Err(error) => error,
    };

    let serde_json::Value::String(encoded) = args else {
        return Err(original_error);
    };

    // Try full string first (simple double-encoding).
    if let Ok(parsed) = serde_json::from_str::<T>(encoded) {
        return Ok(parsed);
    }

    // Some providers concatenate multiple JSON objects in one string.
    // Use a streaming deserializer to extract the first valid value.
    let mut streaming = serde_json::Deserializer::from_str(encoded).into_iter::<T>();
    if let Some(first_result) = streaming.next() {
        let parsed = first_result?;
        if streaming.next().is_some() {
            tracing::warn!("concatenated JSON tool arguments detected; using only first object");
        }
        return Ok(parsed);
    }

    Err(original_error)
}

#[derive(Debug, Deserialize)]
struct ReportFindingsArgs {
    findings: Vec<AnalysisFinding>,
}

fn render_signal_counts(signals: &[SessionSignal]) -> String {
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (session_id, signal) in signals {
        let key = (session_id.clone(), signal.kind.to_string());
        *counts.entry(key).or_insert(0) += 1;
    }

    if counts.is_empty() {
        return "- none".to_string();
    }

    counts
        .into_iter()
        .map(|((session_id, kind), count)| {
            format!("- session_id={session_id} kind={kind} count={count}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_recent_signals(signals: &[SessionSignal], kind: SignalKind, limit: usize) -> String {
    let rows = signals
        .iter()
        .rev()
        .filter(|(_session_id, signal)| signal.kind == kind)
        .take(limit)
        .map(format_signal_row)
        .collect::<Vec<_>>();

    if rows.is_empty() {
        "- none".to_string()
    } else {
        rows.join("\n")
    }
}

fn format_signal_row((session_id, signal): &SessionSignal) -> String {
    let message = sanitize_message(&signal.message);
    format!(
        "- session_id={} ts={} step={} kind={} message={}",
        session_id, signal.timestamp_ms, signal.step, signal.kind, message
    )
}

fn sanitize_message(message: &str) -> String {
    let clean = message.replace('\n', " ");
    if clean.trim().is_empty() {
        "<empty>".to_string()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Confidence;
    use async_trait::async_trait;
    use fx_core::signals::LoopStep;
    use fx_llm::{CompletionResponse, CompletionStream, ContentBlock, ProviderCapabilities};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct MockCompletionProvider {
        model: String,
        result: Result<CompletionResponse, ProviderError>,
        calls: AtomicUsize,
        last_request: Mutex<Option<CompletionRequest>>,
    }

    impl MockCompletionProvider {
        fn success(model: &str, tool_calls: Vec<ToolCall>) -> Self {
            let response = CompletionResponse {
                content: Vec::new(),
                tool_calls,
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            };
            Self::with_result(model, Ok(response))
        }

        fn failure(model: &str, error: ProviderError) -> Self {
            Self::with_result(model, Err(error))
        }

        fn with_result(model: &str, result: Result<CompletionResponse, ProviderError>) -> Self {
            Self {
                model: model.to_string(),
                result,
                calls: AtomicUsize::new(0),
                last_request: Mutex::new(None),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_request(&self) -> Option<CompletionRequest> {
            self.last_request.lock().expect("request lock").clone()
        }
    }

    #[async_trait]
    impl CompletionProvider for MockCompletionProvider {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_request.lock().expect("request lock") = Some(request);
            self.result.clone()
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            Err(ProviderError::Provider(
                "streaming is not supported in tests".to_string(),
            ))
        }

        fn name(&self) -> &str {
            "mock-provider"
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: true,
                requires_streaming: false,
            }
        }
    }

    fn mk_signal(step: LoopStep, kind: SignalKind, message: &str, timestamp_ms: u64) -> Signal {
        Signal {
            step,
            kind,
            message: message.to_string(),
            metadata: json!({}),
            timestamp_ms,
        }
    }

    fn mk_session_signal(session_id: &str, signal: Signal) -> SessionSignal {
        (session_id.to_string(), signal)
    }

    fn sample_finding_json(pattern_name: &str, session_id: &str) -> serde_json::Value {
        json!({
            "pattern_name": pattern_name,
            "description": "Repeated timeout while searching",
            "confidence": "high",
            "evidence": [{
                "session_id": session_id,
                "signal_kind": "friction",
                "message": "tool timeout",
                "timestamp_ms": 1
            }],
            "suggested_action": "Increase timeout budget"
        })
    }

    fn report_findings_call(pattern_name: &str, session_id: &str) -> ToolCall {
        report_findings_call_with_arguments(json!({
            "findings": [sample_finding_json(pattern_name, session_id)]
        }))
    }

    fn report_findings_call_with_arguments(arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: REPORT_FINDINGS_TOOL_NAME.to_string(),
            arguments,
        }
    }

    fn first_message_text(request: &CompletionRequest) -> Option<String> {
        let message = request.messages.first()?;
        let block = message.content.first()?;
        match block {
            ContentBlock::Text { text } => Some(text.clone()),
            ContentBlock::Image { .. } => None,
            _ => None,
        }
    }

    #[test]
    fn report_findings_tool_definition_contains_findings_schema() {
        let tool = report_findings_tool_definition();

        assert_eq!(tool.name, REPORT_FINDINGS_TOOL_NAME);
        assert_eq!(tool.parameters["type"], json!("object"));
        assert_eq!(
            tool.parameters["properties"]["findings"]["type"],
            json!("array")
        );
    }

    #[test]
    fn signal_kind_values_are_non_empty_labels() {
        let values = signal_kind_values();

        assert!(!values.is_empty());
        for label in values {
            assert!(!label.is_empty(), "signal kind label must not be empty");
            assert!(
                label.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "signal kind label '{label}' must be snake_case"
            );
        }
    }

    #[test]
    fn build_signal_summary_includes_sections_without_prompt_instructions() {
        let signals = vec![
            mk_session_signal(
                "session-a",
                mk_signal(LoopStep::Act, SignalKind::Friction, "timeout", 10),
            ),
            mk_session_signal(
                "session-b",
                mk_signal(LoopStep::Act, SignalKind::Success, "completed", 20),
            ),
        ];

        let summary = build_signal_summary(&signals);

        assert!(summary.contains("Signal counts by kind and session"));
        assert!(summary.contains("session_id=session-a kind=friction count=1"));
        assert!(summary.contains("Recent blocked signals"));
        assert!(!summary.contains("Return ONLY"));
        assert!(!summary.contains("pattern_name"));
    }

    #[test]
    fn render_signal_counts_returns_none_for_empty_signals() {
        let signals: Vec<SessionSignal> = Vec::new();
        let counts = render_signal_counts(&signals);
        assert_eq!(counts, "- none");
    }

    #[test]
    fn render_recent_signals_respects_limit_order_and_session_context() {
        let signals = vec![
            mk_session_signal(
                "session-a",
                mk_signal(LoopStep::Act, SignalKind::Friction, "oldest", 1),
            ),
            mk_session_signal(
                "session-b",
                mk_signal(LoopStep::Act, SignalKind::Friction, "middle", 2),
            ),
            mk_session_signal(
                "session-c",
                mk_signal(LoopStep::Act, SignalKind::Friction, "latest", 3),
            ),
        ];

        let rendered = render_recent_signals(&signals, SignalKind::Friction, 2);
        let lines = rendered.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("session_id=session-c"));
        assert!(lines[1].contains("session_id=session-b"));
    }

    #[test]
    fn tool_call_args_normal_object() {
        let calls = vec![
            ToolCall {
                id: "ignore".to_string(),
                name: "different_tool".to_string(),
                arguments: json!({"ignored": true}),
            },
            report_findings_call("Timeout loop", "session-a"),
        ];

        let findings = parse_tool_call_findings(&calls).expect("parse findings");

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern_name, "Timeout loop");
        assert_eq!(findings[0].confidence, Confidence::High);
    }

    #[test]
    fn tool_call_args_double_encoded_json_string() {
        let arguments = json!({
            "findings": [sample_finding_json("Timeout loop", "session-a")]
        });
        let call = report_findings_call_with_arguments(serde_json::Value::String(
            serde_json::to_string(&arguments).expect("serialized args"),
        ));

        let findings = parse_tool_call_findings(&[call]).expect("parse findings");

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern_name, "Timeout loop");
    }

    #[test]
    fn tool_call_args_concatenated_json_objects() {
        let arguments = json!({
            "findings": [sample_finding_json("Timeout loop", "session-a")]
        });
        let serialized = serde_json::to_string(&arguments).expect("serialized args");
        let call = report_findings_call_with_arguments(serde_json::Value::String(format!(
            "{serialized}{serialized}{serialized}"
        )));

        let findings = parse_tool_call_findings(&[call]).expect("parse findings");

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern_name, "Timeout loop");
    }

    #[test]
    fn duplicate_findings_deduplicated() {
        let calls = vec![
            report_findings_call("Timeout loop", "session-a"),
            report_findings_call("Timeout loop", "session-a"),
        ];

        let findings = parse_tool_call_findings(&calls).expect("parse findings");

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern_name, "Timeout loop");
    }

    #[test]
    fn tool_call_args_invalid_string() {
        let calls = vec![report_findings_call_with_arguments(
            serde_json::Value::String("not json at all".to_string()),
        )];

        let error = parse_tool_call_findings(&calls).expect_err("expected parse failure");

        assert!(matches!(error, AnalysisError::ParseError(_)));
    }

    #[test]
    fn parse_tool_call_findings_returns_parse_error_for_bad_arguments() {
        let calls = vec![ToolCall {
            id: "bad".to_string(),
            name: REPORT_FINDINGS_TOOL_NAME.to_string(),
            arguments: json!({"findings": "not-an-array"}),
        }];

        let error = parse_tool_call_findings(&calls).expect_err("expected parse failure");

        assert!(matches!(error, AnalysisError::ParseError(_)));
    }

    #[tokio::test]
    async fn analysis_with_no_signals_returns_empty_findings_without_llm_call() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "empty-session").expect("store");

        let provider = MockCompletionProvider::success("gpt-4o", Vec::new());

        let engine = AnalysisEngine::new(&store);
        let findings = engine.analyze(&provider).await.expect("analyze");

        assert!(findings.is_empty());
        assert_eq!(provider.call_count(), 0);
    }

    #[tokio::test]
    async fn analysis_uses_tool_calls_and_returns_findings() {
        let tmp = TempDir::new().expect("tempdir");
        let store_a = SignalStore::new(tmp.path(), "session-a").expect("store a");
        let store_b = SignalStore::new(tmp.path(), "session-b").expect("store b");
        store_a
            .persist(&[mk_signal(
                LoopStep::Act,
                SignalKind::Friction,
                "tool timeout",
                1,
            )])
            .expect("persist a");
        store_b
            .persist(&[mk_signal(LoopStep::Act, SignalKind::Success, "tool ok", 2)])
            .expect("persist b");

        let provider = MockCompletionProvider::success(
            "gpt-4o",
            vec![report_findings_call("Timeout loop", "session-a")],
        );
        let engine = AnalysisEngine::new(&store_a);

        let findings = engine.analyze(&provider).await.expect("analyze");
        let request = provider.last_request().expect("captured request");
        let summary = first_message_text(&request).expect("user summary");

        assert_eq!(provider.call_count(), 1);
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, REPORT_FINDINGS_TOOL_NAME);
        assert_eq!(
            request.system_prompt.as_deref(),
            Some(ANALYSIS_SYSTEM_PROMPT)
        );
        assert!(summary.contains("session_id=session-a"));
        assert!(summary.contains("session_id=session-b"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern_name, "Timeout loop");
    }

    #[tokio::test]
    async fn analysis_propagates_provider_errors() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "error-session").expect("store");
        store
            .persist(&[mk_signal(LoopStep::Act, SignalKind::Friction, "failure", 1)])
            .expect("persist");

        let provider =
            MockCompletionProvider::failure("gpt-4o", ProviderError::Provider("boom".to_string()));

        let engine = AnalysisEngine::new(&store);
        let error = engine
            .analyze(&provider)
            .await
            .expect_err("expected provider failure");

        assert!(matches!(error, AnalysisError::Llm(_)));
    }

    #[tokio::test]
    async fn analysis_returns_parse_error_for_invalid_tool_payload() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "parse-session").expect("store");
        store
            .persist(&[mk_signal(LoopStep::Act, SignalKind::Friction, "failure", 1)])
            .expect("persist");

        let bad_call = ToolCall {
            id: "bad-parse".to_string(),
            name: REPORT_FINDINGS_TOOL_NAME.to_string(),
            arguments: json!({"findings": [{"pattern_name": 123}]}),
        };
        let provider = MockCompletionProvider::success("gpt-4o", vec![bad_call]);

        let engine = AnalysisEngine::new(&store);
        let error = engine
            .analyze(&provider)
            .await
            .expect_err("expected parse error");

        assert!(matches!(error, AnalysisError::ParseError(_)));
    }
}

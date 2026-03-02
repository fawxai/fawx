use fx_core::error::LlmError;
use fx_core::signals::{Signal, SignalKind};
use fx_kernel::loop_engine::LlmProvider;
use fx_memory::signal_store::SignalStoreError;
use fx_memory::{AnalysisFinding, SignalStore};
use std::collections::BTreeMap;
use std::fmt;

const ANALYSIS_JSON_SCHEMA: &str = r#"[
  {
    "pattern_name": "...",
    "description": "...",
    "confidence": "high",
    "evidence": [
      {
        "session_id": "...",
        "signal_kind": "friction",
        "message": "...",
        "timestamp_ms": 0
      }
    ],
    "suggested_action": "..."
  }
]
"#;

type SessionSignal = (String, Signal);

#[derive(Debug)]
pub enum AnalysisError {
    SignalStore(SignalStoreError),
    Llm(LlmError),
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

impl From<LlmError> for AnalysisError {
    fn from(value: LlmError) -> Self {
        Self::Llm(value)
    }
}

#[derive(Debug)]
pub struct AnalysisEngine<'a> {
    signal_store: &'a SignalStore,
}

impl<'a> AnalysisEngine<'a> {
    pub fn new(signal_store: &'a SignalStore) -> Self {
        Self { signal_store }
    }

    pub async fn analyze(
        &self,
        llm: &dyn LlmProvider,
    ) -> Result<Vec<AnalysisFinding>, AnalysisError> {
        let signals = self.signal_store.load_all()?;
        if signals.is_empty() {
            return Ok(Vec::new());
        }

        let prompt = self.build_prompt(&signals);
        let response = llm.generate(&prompt, 4096).await?;
        self.parse_findings(&response)
    }

    fn build_prompt(&self, signals: &[SessionSignal]) -> String {
        let counts = render_signal_counts(signals);
        let friction = render_recent_signals(signals, SignalKind::Friction, 8);
        let blocked = render_recent_signals(signals, SignalKind::Blocked, 8);
        let success = render_recent_signals(signals, SignalKind::Success, 8);

        format!(
            "You are analyzing agent runtime signals captured across sessions.

Signal counts by kind and session:
{counts}

Recent friction signals:
{friction}

Recent blocked signals:
{blocked}

Recent success signals:
{success}

Identify recurring patterns that emerge from these signals.
For each pattern, provide:
- pattern_name
- description
- confidence (high, medium, low)
- evidence (array of objects with session_id, signal_kind, message, timestamp_ms)
- suggested_action (optional)

Return ONLY a JSON array that matches this schema exactly:
{ANALYSIS_JSON_SCHEMA}"
        )
    }

    fn parse_findings(&self, response: &str) -> Result<Vec<AnalysisFinding>, AnalysisError> {
        if let Ok(findings) = serde_json::from_str::<Vec<AnalysisFinding>>(response) {
            return Ok(findings);
        }

        let Some(json_array) = extract_json_array(response) else {
            return Ok(Vec::new());
        };

        parse_findings_from_json_array(&json_array)
    }
}

fn parse_findings_from_json_array(json_array: &str) -> Result<Vec<AnalysisFinding>, AnalysisError> {
    serde_json::from_str::<Vec<AnalysisFinding>>(json_array).map_err(|error| {
        tracing::warn!("analysis response contained unparseable findings JSON: {error}");
        AnalysisError::ParseError(error)
    })
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

fn extract_json_array(response: &str) -> Option<String> {
    extract_json_code_block(response).or_else(|| extract_balanced_array(response))
}

fn extract_json_code_block(response: &str) -> Option<String> {
    let mut parts = response.split("```");
    while let Some(_prefix) = parts.next() {
        let block = parts.next()?;
        if let Some(payload) = json_payload_from_fenced_block(block) {
            return Some(payload);
        }
    }
    None
}

fn json_payload_from_fenced_block(block: &str) -> Option<String> {
    if let Some((first_line, rest)) = block.trim().split_once('\n') {
        if first_line.trim().eq_ignore_ascii_case("json") {
            return Some(rest.trim().to_string());
        }
    }

    let trimmed = strip_code_fence_language(block).trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn strip_code_fence_language(block: &str) -> &str {
    let trimmed = block.trim();
    let Some((first_line, rest)) = trimmed.split_once('\n') else {
        return trimmed;
    };

    if is_fence_language(first_line) {
        rest
    } else {
        trimmed
    }
}

fn is_fence_language(line: &str) -> bool {
    !line.is_empty()
        && line
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn extract_balanced_array(response: &str) -> Option<String> {
    let start = response.find('[')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in response[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + index + 1;
                    return Some(response[start..end].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::signals::LoopStep;
    use fx_memory::Confidence;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct MockLlm {
        response: String,
        calls: AtomicUsize,
        last_prompt: Mutex<Option<String>>,
    }

    impl MockLlm {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_string(),
                calls: AtomicUsize::new(0),
                last_prompt: Mutex::new(None),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn capture_prompt(&self, prompt: &str) {
            let mut guard = self.last_prompt.lock().expect("prompt lock");
            *guard = Some(prompt.to_string());
        }

        fn last_prompt(&self) -> Option<String> {
            self.last_prompt.lock().expect("prompt lock").clone()
        }
    }

    #[async_trait]
    impl LlmProvider for MockLlm {
        async fn generate(&self, prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.capture_prompt(prompt);
            Ok(self.response.clone())
        }

        async fn generate_streaming(
            &self,
            prompt: &str,
            _max_tokens: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.capture_prompt(prompt);
            callback(self.response.clone());
            Ok(self.response.clone())
        }

        fn model_name(&self) -> &str {
            "mock-model"
        }
    }

    fn mk_signal(step: LoopStep, kind: SignalKind, message: &str, timestamp_ms: u64) -> Signal {
        Signal {
            step,
            kind,
            message: message.to_string(),
            metadata: serde_json::json!({}),
            timestamp_ms,
        }
    }

    fn mk_session_signal(
        session_id: &str,
        step: LoopStep,
        kind: SignalKind,
        message: &str,
        timestamp_ms: u64,
    ) -> SessionSignal {
        (
            session_id.to_string(),
            mk_signal(step, kind, message, timestamp_ms),
        )
    }

    #[test]
    fn prompt_construction_includes_session_aware_counts_and_details() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "prompt-session").expect("new store");
        let engine = AnalysisEngine::new(&store);

        let signals = vec![
            mk_session_signal(
                "session-a",
                LoopStep::Act,
                SignalKind::Friction,
                "timeout while searching",
                100,
            ),
            mk_session_signal(
                "session-a",
                LoopStep::Act,
                SignalKind::Blocked,
                "permission denied",
                200,
            ),
            mk_session_signal(
                "session-b",
                LoopStep::Act,
                SignalKind::Success,
                "search completed",
                300,
            ),
        ];

        let prompt = engine.build_prompt(&signals);

        assert!(prompt.contains("Signal counts by kind and session:"));
        assert!(prompt.contains("session_id=session-a kind=blocked count=1"));
        assert!(prompt.contains("session_id=session-a kind=friction count=1"));
        assert!(prompt.contains("session_id=session-b kind=success count=1"));
        assert!(prompt.contains("Recent blocked signals:"));
        assert!(prompt.contains("session_id=session-a"));
        assert!(prompt.contains("permission denied"));
    }

    #[test]
    fn render_signal_counts_returns_none_for_empty_signals() {
        let signals: Vec<SessionSignal> = Vec::new();
        let counts = render_signal_counts(&signals);
        assert_eq!(counts, "- none");
    }

    #[test]
    fn render_signal_counts_aggregates_per_session_and_kind() {
        let signals = vec![
            mk_session_signal("session-a", LoopStep::Act, SignalKind::Friction, "one", 1),
            mk_session_signal(
                "session-a",
                LoopStep::Reason,
                SignalKind::Friction,
                "two",
                2,
            ),
            mk_session_signal("session-b", LoopStep::Act, SignalKind::Friction, "three", 3),
        ];

        let counts = render_signal_counts(&signals);

        assert!(counts.contains("session_id=session-a kind=friction count=2"));
        assert!(counts.contains("session_id=session-b kind=friction count=1"));
    }

    #[test]
    fn render_recent_signals_returns_none_when_kind_is_missing() {
        let signals = vec![mk_session_signal(
            "session-a",
            LoopStep::Act,
            SignalKind::Success,
            "ok",
            1,
        )];
        let rendered = render_recent_signals(&signals, SignalKind::Blocked, 3);
        assert_eq!(rendered, "- none");
    }

    #[test]
    fn render_recent_signals_respects_limit_order_and_session_context() {
        let signals = vec![
            mk_session_signal(
                "session-a",
                LoopStep::Act,
                SignalKind::Friction,
                "oldest",
                1,
            ),
            mk_session_signal(
                "session-b",
                LoopStep::Act,
                SignalKind::Friction,
                "middle",
                2,
            ),
            mk_session_signal(
                "session-c",
                LoopStep::Act,
                SignalKind::Friction,
                "latest",
                3,
            ),
        ];

        let rendered = render_recent_signals(&signals, SignalKind::Friction, 2);
        let lines = rendered.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("session_id=session-c"));
        assert!(lines[0].contains("latest"));
        assert!(lines[1].contains("session_id=session-b"));
        assert!(lines[1].contains("middle"));
    }

    #[test]
    fn strip_code_fence_language_handles_language_and_plain_blocks() {
        assert_eq!(strip_code_fence_language("json\n[]"), "[]");
        assert_eq!(strip_code_fence_language("[]"), "[]");
        assert_eq!(strip_code_fence_language(""), "");
    }

    #[test]
    fn extract_json_code_block_handles_empty_and_unfenced_input() {
        assert_eq!(extract_json_code_block(""), None);
        assert_eq!(extract_json_code_block("[{}]"), None);
    }

    #[test]
    fn extract_json_code_block_uses_json_block_when_multiple_fences_exist() {
        let response = r#"```text
ignore this
```
Some commentary.
```json
[{"pattern_name":"x"}]
```
```yaml
foo: bar
```
"#;

        let extracted = extract_json_code_block(response).expect("json code block");
        assert_eq!(extracted, r#"[{"pattern_name":"x"}]"#);
    }

    #[test]
    fn extract_balanced_array_handles_nested_arrays_and_string_brackets() {
        let response =
            r#"prefix [{"nested":[{"values":[1,2]}],"message":"contains [brackets]"}] suffix"#;
        let extracted = extract_balanced_array(response).expect("balanced array");

        assert_eq!(
            extracted,
            r#"[{"nested":[{"values":[1,2]}],"message":"contains [brackets]"}]"#
        );
    }

    #[test]
    fn extract_balanced_array_returns_none_for_unclosed_array() {
        assert!(extract_balanced_array(r#"prefix [{"pattern":"oops"}"#).is_none());
    }

    #[test]
    fn parse_findings_returns_empty_when_response_has_no_json() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "parse-session").expect("new store");
        let engine = AnalysisEngine::new(&store);

        let findings = engine
            .parse_findings("No structured output was produced.")
            .expect("non-json text should be treated as no findings");

        assert!(findings.is_empty());
    }

    #[test]
    fn parse_findings_returns_error_for_malformed_json_code_block() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "parse-session").expect("new store");
        let engine = AnalysisEngine::new(&store);

        let response = r#"```json
[{"pattern_name":"oops"}
```"#;
        let error = engine
            .parse_findings(response)
            .expect_err("partial JSON should fail parsing");

        assert!(matches!(error, AnalysisError::ParseError(_)));
    }

    #[test]
    fn parse_findings_returns_error_for_wrong_schema_json() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "parse-session").expect("new store");
        let engine = AnalysisEngine::new(&store);

        let response = r#"```json
[{"pattern_name":123}]
```"#;
        let error = engine
            .parse_findings(response)
            .expect_err("schema mismatch should fail parsing");

        assert!(matches!(error, AnalysisError::ParseError(_)));
    }

    #[tokio::test]
    async fn analysis_with_no_signals_returns_empty_findings() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "empty-session").expect("new store");
        let engine = AnalysisEngine::new(&store);
        let llm = MockLlm::new("[]");

        let findings = engine.analyze(&llm).await.expect("analyze");

        assert!(findings.is_empty());
        assert_eq!(llm.call_count(), 0);
    }

    #[tokio::test]
    async fn analysis_with_mock_signals_produces_structured_output() {
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
            .persist(&[mk_signal(
                LoopStep::Act,
                SignalKind::Success,
                "tool success",
                2,
            )])
            .expect("persist b");

        let llm = MockLlm::new(
            r#"```json
[
  {
    "pattern_name": "Timeout loop",
    "description": "Repeated tool timeout friction",
    "confidence": "high",
    "evidence": [
      {
        "session_id": "session-a",
        "signal_kind": "friction",
        "message": "tool timeout",
        "timestamp_ms": 1
      }
    ],
    "suggested_action": "Increase timeout budget"
  }
]
```"#,
        );
        let engine = AnalysisEngine::new(&store_a);

        let findings = engine.analyze(&llm).await.expect("analyze");
        let prompt = llm.last_prompt().expect("captured prompt");

        assert_eq!(llm.call_count(), 1);
        assert!(prompt.contains("session_id=session-a"));
        assert!(prompt.contains("session_id=session-b"));
        assert!(prompt.contains("tool timeout"));
        assert!(prompt.contains("tool success"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern_name, "Timeout loop");
        assert_eq!(findings[0].confidence, Confidence::High);
        assert_eq!(findings[0].evidence.len(), 1);
        assert_eq!(
            findings[0].suggested_action.as_deref(),
            Some("Increase timeout budget")
        );
    }
}

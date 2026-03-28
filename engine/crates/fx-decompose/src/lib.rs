pub mod aggregator;
pub mod context;
pub mod dag;
pub mod dispatcher;
pub mod engine;
pub mod error;

use fx_core::signals::Signal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComplexityHint {
    Trivial,
    Moderate,
    Complex,
}

impl ComplexityHint {
    pub const fn weight(self) -> u32 {
        match self {
            Self::Trivial => 1,
            Self::Moderate => 2,
            Self::Complex => 4,
        }
    }
}

const COMPLETION_STOP_WORDS: &[&str] = &[
    "a", "an", "and", "for", "from", "into", "of", "on", "or", "the", "to", "with", "output",
    "result",
];

const META_ONLY_RESPONSE_STARTERS: &[&str] = &[
    "let me",
    "i'll",
    "i will",
    "i need",
    "need direction",
    "before i can finish",
    "i'm going to",
    "going to",
    "next i",
    "first i",
];

const META_ONLY_RESPONSE_PHRASES: &[&str] = &[
    "need direction",
    "blocked",
    "before i can finish",
    "can't proceed",
    "cannot proceed",
    "if you want, i can",
    "not enough information",
    "need more information",
    "need follow-up",
    "still gathering",
    "still researching",
    "parallelize",
    "would you like me to",
];

const ACTION_ORIENTED_TASK_TERMS: &[&str] = &[
    "build",
    "create",
    "fix",
    "generate",
    "implement",
    "install",
    "modify",
    "patch",
    "post",
    "publish",
    "save",
    "scaffold",
    "update",
    "write",
];

const UNRESOLVED_ACTION_RESPONSE_PHRASES: &[&str] = &[
    "what went wrong",
    "still need to",
    "no such file or directory",
    "command not found",
    "permission denied",
    "timed out",
    "failed to",
    "could not",
    "couldn't",
    "unable to",
    "cannot",
    "can't",
    "unsupported",
    "not found",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubGoalDescription {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubGoalContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_terms: Vec<String>,
    #[serde(default = "default_require_substantive_text")]
    pub require_substantive_text: bool,
    #[serde(default = "default_reject_meta_only")]
    pub reject_meta_only: bool,
}

impl Default for SubGoalContract {
    fn default() -> Self {
        Self {
            definition_of_done: None,
            required_terms: Vec::new(),
            require_substantive_text: false,
            reject_meta_only: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubGoalCompletionClassification {
    Completed,
    Incomplete(String),
}

pub trait ExecutionContract<Evidence: ?Sized> {
    type Description;
    type Classification;

    fn describe(&self) -> Self::Description;
    fn classify(&self, evidence: &Evidence) -> Self::Classification;
}

fn default_require_substantive_text() -> bool {
    true
}

fn default_reject_meta_only() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct SubGoalWire {
    pub description: String,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub completion_contract: SubGoalContract,
    #[serde(
        rename = "expected_output",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub legacy_definition_of_done: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_hint: Option<ComplexityHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(from = "SubGoalWire", into = "SubGoalWire")]
pub struct SubGoal {
    pub description: String,
    pub required_tools: Vec<String>,
    pub completion_contract: SubGoalContract,
    pub complexity_hint: Option<ComplexityHint>,
}

impl SubGoal {
    pub fn contract(&self) -> SubGoalContract {
        self.completion_contract.prompt_contract(&self.description)
    }

    pub fn new(
        description: impl Into<String>,
        required_tools: Vec<String>,
        completion_contract: SubGoalContract,
        complexity_hint: Option<ComplexityHint>,
    ) -> Self {
        Self {
            description: description.into(),
            required_tools,
            completion_contract,
            complexity_hint,
        }
    }

    pub fn with_definition_of_done(
        description: impl Into<String>,
        required_tools: Vec<String>,
        definition_of_done: Option<&str>,
        complexity_hint: Option<ComplexityHint>,
    ) -> Self {
        Self::new(
            description,
            required_tools,
            SubGoalContract::from_definition_of_done(definition_of_done),
            complexity_hint,
        )
    }
}

impl From<SubGoalWire> for SubGoal {
    fn from(value: SubGoalWire) -> Self {
        let completion_contract = value
            .completion_contract
            .merge_legacy_definition_of_done(value.legacy_definition_of_done.as_deref());

        SubGoal {
            description: value.description,
            required_tools: value.required_tools,
            completion_contract,
            complexity_hint: value.complexity_hint,
        }
    }
}

impl From<SubGoal> for SubGoalWire {
    fn from(value: SubGoal) -> Self {
        SubGoalWire {
            description: value.description,
            required_tools: value.required_tools,
            completion_contract: value.completion_contract,
            legacy_definition_of_done: None,
            complexity_hint: value.complexity_hint,
        }
    }
}

impl ExecutionContract<str> for SubGoal {
    type Description = SubGoalDescription;
    type Classification = SubGoalCompletionClassification;

    fn describe(&self) -> Self::Description {
        self.contract().describe_with_task(&self.description)
    }

    fn classify(&self, evidence: &str) -> Self::Classification {
        let normalized = evidence.trim();
        if looks_unresolved_action_response(&self.description, normalized) {
            return SubGoalCompletionClassification::Incomplete(format!(
                "sub-goal response reported unresolved execution blockers instead of completed work: {normalized}"
            ));
        }

        self.completion_contract
            .classification_contract(&self.description)
            .classify(evidence)
    }
}

impl SubGoalContract {
    pub fn from_definition_of_done(definition_of_done: Option<&str>) -> Self {
        let definition_of_done = definition_of_done
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned);

        let required_terms = definition_of_done
            .as_deref()
            .map(salient_terms)
            .unwrap_or_default();
        let has_definition = definition_of_done.is_some();

        Self {
            definition_of_done,
            required_terms,
            require_substantive_text: has_definition,
            reject_meta_only: true,
        }
    }

    pub fn is_effectively_empty(&self) -> bool {
        self.definition_of_done.is_none()
            && self.required_terms.is_empty()
            && !self.require_substantive_text
            && self.reject_meta_only
    }

    fn merge_legacy_definition_of_done(&self, definition_of_done: Option<&str>) -> Self {
        let legacy = Self::from_definition_of_done(definition_of_done);
        let legacy_is_empty = legacy.is_effectively_empty();
        if self.is_effectively_empty() {
            return legacy;
        }

        let mut merged = self.clone();
        if merged.definition_of_done.is_none() {
            merged.definition_of_done = legacy.definition_of_done;
        }

        if merged.required_terms.is_empty() {
            merged.required_terms = if legacy.required_terms.is_empty() {
                merged
                    .definition_of_done
                    .as_deref()
                    .map(salient_terms)
                    .unwrap_or_default()
            } else {
                legacy.required_terms.clone()
            };
        }

        if merged.definition_of_done.is_some()
            && !merged.require_substantive_text
            && !legacy_is_empty
        {
            merged.require_substantive_text = true;
        }

        merged
    }

    fn prompt_contract(&self, description: &str) -> Self {
        if self.definition_of_done.is_some() || !self.required_terms.is_empty() {
            return self.clone();
        }

        self.with_task_terms(description)
    }

    fn classification_contract(&self, description: &str) -> Self {
        self.with_task_terms(description)
    }

    fn with_task_terms(&self, description: &str) -> Self {
        let task_terms = salient_terms(description);
        if task_terms.is_empty() {
            return self.clone();
        }

        let mut merged = self.clone();
        for term in task_terms {
            if !merged.required_terms.contains(&term) {
                merged.required_terms.push(term);
            }
        }
        if !merged.required_terms.is_empty() {
            merged.require_substantive_text = true;
        }
        merged
    }

    pub fn describe_with_task(&self, description: &str) -> SubGoalDescription {
        let mut prompt = description.trim().to_string();

        if let Some(definition_of_done) = self.definition_of_done.as_deref() {
            prompt.push_str("\n\nDefinition of done:\n- ");
            prompt.push_str(definition_of_done);
        }

        if !self.required_terms.is_empty() {
            prompt.push_str("\n\nCompletion evidence to include in the final response:");
            for term in &self.required_terms {
                prompt.push_str("\n- ");
                prompt.push_str(term);
            }
        }

        SubGoalDescription { prompt }
    }
}

impl ExecutionContract<str> for SubGoalContract {
    type Description = SubGoalDescription;
    type Classification = SubGoalCompletionClassification;

    fn describe(&self) -> Self::Description {
        self.describe_with_task("")
    }

    fn classify(&self, evidence: &str) -> Self::Classification {
        let normalized = evidence.trim();
        if self.require_substantive_text && normalized.is_empty() {
            return SubGoalCompletionClassification::Incomplete(
                "sub-goal returned no completion evidence".to_string(),
            );
        }

        if self.require_substantive_text && normalized.len() < 3 {
            return SubGoalCompletionClassification::Incomplete(format!(
                "sub-goal response was too short to prove completion: {normalized}"
            ));
        }

        if self.reject_meta_only && looks_meta_only_response(normalized) {
            return SubGoalCompletionClassification::Incomplete(format!(
                "sub-goal response described next steps instead of completed work: {normalized}"
            ));
        }

        if !self.required_terms.is_empty() {
            let matched = self
                .required_terms
                .iter()
                .filter(|term| response_matches_required_term(normalized, term))
                .count();
            let required_matches = minimum_required_term_matches(self.required_terms.len());
            if matched < required_matches {
                return SubGoalCompletionClassification::Incomplete(format!(
                    "sub-goal response did not include enough completion evidence markers (matched {matched}/{required_matches} needed from [{}]): {normalized}",
                    self.required_terms.join(", ")
                ));
            }
        }

        SubGoalCompletionClassification::Completed
    }
}

fn salient_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();

    for token in text
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
    {
        if COMPLETION_STOP_WORDS.contains(&token.as_str()) || terms.contains(&token) {
            continue;
        }
        terms.push(token);
    }

    terms
}

fn looks_meta_only_response(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();

    META_ONLY_RESPONSE_STARTERS
        .iter()
        .any(|pattern| normalized.starts_with(pattern))
        || META_ONLY_RESPONSE_PHRASES
            .iter()
            .any(|pattern| normalized.contains(pattern))
}

fn response_matches_required_term(text: &str, term: &str) -> bool {
    let normalized_text = text.to_ascii_lowercase();
    let normalized_term = term.to_ascii_lowercase();
    if normalized_text.contains(&normalized_term) {
        return true;
    }

    normalized_text
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(|token| {
            let shared_prefix = token
                .chars()
                .zip(normalized_term.chars())
                .take_while(|(left, right)| left == right)
                .count();
            shared_prefix >= 5
        })
}

fn minimum_required_term_matches(term_count: usize) -> usize {
    term_count.min(2)
}

fn looks_unresolved_action_response(task: &str, response: &str) -> bool {
    if response.is_empty() {
        return false;
    }

    let normalized_task = task.trim().to_ascii_lowercase();
    if !ACTION_ORIENTED_TASK_TERMS
        .iter()
        .any(|term| normalized_task.contains(term))
    {
        return false;
    }

    let normalized_response = response.trim().to_ascii_lowercase();
    UNRESOLVED_ACTION_RESPONSE_PHRASES
        .iter()
        .any(|pattern| normalized_response.contains(pattern))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecompositionPlan {
    pub sub_goals: Vec<SubGoal>,
    pub strategy: AggregationStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated_from: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AggregationStrategy {
    Sequential,
    Parallel,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubGoalResult {
    pub goal: SubGoal,
    pub outcome: SubGoalOutcome,
    pub signals: Vec<Signal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubGoalOutcome {
    Completed(String),
    Incomplete(String),
    Failed(String),
    BudgetExhausted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        partial_response: Option<String>,
    },
    Skipped,
}

pub use aggregator::{
    AggregatedResult, BuildVerifyAggregator, DefaultWorkspaceProvider, MergeResult,
    MergeTestResult, PatchApplyError, ResultAggregator, SimpleAggregator, TempWorkspace,
    WorkspaceProvider,
};
pub use context::{
    AttemptDecision, ChainEntry, DecompositionAttempt, DecompositionContext, Experiment,
    FitnessContext, FitnessStats, PathPattern, SubGoalAttempt, SubGoalAttemptOutcome,
};
pub use dag::ExecutionDag;
#[cfg(any(test, feature = "test-support"))]
pub use dispatcher::MockSubGoalExecutor;
pub use dispatcher::{
    DagDispatcher, DecompositionEvent, DecompositionProgressCallback, ParallelDispatcher,
    SequentialDispatcher, SubGoalDispatcher, SubGoalExecutor,
};
pub use engine::{
    format_fitness_context, parse_plan_json, validate_plan, Decomposer, LlmDecomposer,
};
pub use error::DecomposeError;

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::signals::{LoopStep, SignalKind};

    fn sample_sub_goal() -> SubGoal {
        SubGoal::with_definition_of_done(
            "Summarize issue history",
            vec!["gh".to_string(), "read_file".to_string()],
            Some("Summary of issue events"),
            Some(ComplexityHint::Moderate),
        )
    }

    fn sample_signal() -> Signal {
        Signal {
            step: LoopStep::Act,
            kind: SignalKind::Success,
            message: "sub-goal completed".to_string(),
            metadata: serde_json::json!({"test": true}),
            timestamp_ms: 42,
        }
    }

    #[test]
    fn sub_goal_roundtrip_serde() {
        let original = sample_sub_goal();
        let encoded = serde_json::to_string(&original).expect("serialize sub-goal");
        let decoded: SubGoal = serde_json::from_str(&encoded).expect("deserialize sub-goal");
        assert_eq!(decoded, original);
    }

    #[test]
    fn decomposition_plan_roundtrip_serde() {
        let original = DecompositionPlan {
            sub_goals: vec![sample_sub_goal()],
            strategy: AggregationStrategy::Sequential,
            truncated_from: None,
        };

        let encoded = serde_json::to_string(&original).expect("serialize plan");
        let decoded: DecompositionPlan = serde_json::from_str(&encoded).expect("deserialize plan");
        assert_eq!(decoded, original);
    }

    #[test]
    fn sub_goal_result_roundtrip_serde() {
        let original = SubGoalResult {
            goal: sample_sub_goal(),
            outcome: SubGoalOutcome::Completed("done".to_string()),
            signals: vec![sample_signal()],
        };

        let encoded = serde_json::to_string(&original).expect("serialize result");
        let decoded: SubGoalResult =
            serde_json::from_str(&encoded).expect("deserialize sub-goal result");
        assert_eq!(decoded, original);
    }

    #[test]
    fn aggregation_strategy_variants_serialize_correctly() {
        let sequential = serde_json::to_value(AggregationStrategy::Sequential).expect("seq");
        let parallel = serde_json::to_value(AggregationStrategy::Parallel).expect("par");
        let custom = serde_json::to_value(AggregationStrategy::Custom("fan-in".to_string()))
            .expect("custom");

        assert_eq!(sequential, serde_json::json!("Sequential"));
        assert_eq!(parallel, serde_json::json!("Parallel"));
        assert_eq!(custom, serde_json::json!({"Custom": "fan-in"}));
    }

    #[test]
    fn sub_goal_outcome_variants_cover_all_cases() {
        let completed = SubGoalOutcome::Completed("ok".to_string());
        let incomplete = SubGoalOutcome::Incomplete("needs more evidence".to_string());
        let failed = SubGoalOutcome::Failed("boom".to_string());
        let exhausted = SubGoalOutcome::BudgetExhausted {
            partial_response: Some("partial".to_string()),
        };
        let skipped = SubGoalOutcome::Skipped;

        assert!(matches!(completed, SubGoalOutcome::Completed(text) if text == "ok"));
        assert!(
            matches!(incomplete, SubGoalOutcome::Incomplete(text) if text == "needs more evidence")
        );
        assert!(matches!(failed, SubGoalOutcome::Failed(text) if text == "boom"));
        assert!(matches!(
            exhausted,
            SubGoalOutcome::BudgetExhausted { partial_response: Some(text) } if text == "partial"
        ));
        assert!(matches!(skipped, SubGoalOutcome::Skipped));
    }

    #[test]
    fn empty_sub_goals_list_roundtrip_serde() {
        let plan = DecompositionPlan {
            sub_goals: Vec::new(),
            strategy: AggregationStrategy::Sequential,
            truncated_from: None,
        };

        let encoded = serde_json::to_string(&plan).expect("serialize empty plan");
        let decoded: DecompositionPlan =
            serde_json::from_str(&encoded).expect("deserialize empty plan");
        assert!(decoded.sub_goals.is_empty());
    }

    #[test]
    fn required_tools_empty_vec_roundtrip_serde() {
        let goal = SubGoal {
            description: "No tool task".to_string(),
            required_tools: Vec::new(),
            completion_contract: SubGoalContract::from_definition_of_done(Some("Plain text")),
            complexity_hint: None,
        };

        let encoded = serde_json::to_string(&goal).expect("serialize goal");
        let decoded: SubGoal = serde_json::from_str(&encoded).expect("deserialize goal");
        assert!(decoded.required_tools.is_empty());
    }

    #[test]
    fn missing_completion_contract_deserializes_to_default_contract() {
        let encoded = serde_json::json!({
            "description": "Summarize findings",
            "required_tools": ["read_file"]
        });

        let decoded: SubGoal = serde_json::from_value(encoded).expect("deserialize goal");

        assert_eq!(decoded.completion_contract, SubGoalContract::default());
    }

    #[test]
    fn legacy_expected_output_deserializes_into_completion_contract() {
        let encoded = serde_json::json!({
            "description": "Summarize findings",
            "required_tools": ["read_file"],
            "expected_output": "summary artifact"
        });

        let decoded: SubGoal = serde_json::from_value(encoded).expect("deserialize goal");
        assert_eq!(
            decoded.completion_contract,
            SubGoalContract::from_definition_of_done(Some("summary artifact"))
        );
    }

    #[test]
    fn legacy_expected_output_alias_is_omitted_from_serialization() {
        let goal = SubGoal::new(
            "Summarize findings",
            Vec::new(),
            SubGoalContract::default(),
            None,
        );

        let encoded = serde_json::to_value(&goal).expect("serialize goal");

        assert!(encoded.get("expected_output").is_none());
    }

    #[test]
    fn sub_goal_with_complexity_hint_roundtrip_serde() {
        let goal = SubGoal::with_definition_of_done(
            "Implement adaptive budget allocator",
            vec!["read_file".to_string()],
            Some("patch"),
            Some(ComplexityHint::Complex),
        );

        let encoded = serde_json::to_string(&goal).expect("serialize sub-goal");
        let decoded: SubGoal = serde_json::from_str(&encoded).expect("deserialize sub-goal");

        assert_eq!(decoded.complexity_hint, Some(ComplexityHint::Complex));
        assert_eq!(decoded, goal);
    }

    #[test]
    fn complexity_hint_weight_values_are_stable() {
        assert_eq!(ComplexityHint::Trivial.weight(), 1);
        assert_eq!(ComplexityHint::Moderate.weight(), 2);
        assert_eq!(ComplexityHint::Complex.weight(), 4);
    }

    #[test]
    fn sub_goal_outcome_skipped_roundtrip_serde() {
        let encoded = serde_json::to_string(&SubGoalOutcome::Skipped).expect("serialize skipped");
        let decoded: SubGoalOutcome = serde_json::from_str(&encoded).expect("deserialize skipped");
        assert_eq!(decoded, SubGoalOutcome::Skipped);
    }

    #[test]
    fn sub_goal_describe_includes_definition_of_done_and_evidence_markers() {
        let description = sample_sub_goal().describe();

        assert!(description.prompt.contains("Definition of done:"));
        assert!(description.prompt.contains("Summary of issue events"));
        assert!(description
            .prompt
            .contains("Completion evidence to include in the final response"));
        assert!(description.prompt.contains("summary"));
        assert!(description.prompt.contains("issue"));
        assert!(description.prompt.contains("events"));
    }

    #[test]
    fn sub_goal_classification_accepts_matching_completion_evidence() {
        let goal = sample_sub_goal();
        let classification =
            goal.classify("Issue events summary written from the fetched timeline.");

        assert_eq!(classification, SubGoalCompletionClassification::Completed);
    }

    #[test]
    fn sub_goal_classification_rejects_meta_only_progress_text() {
        let goal = sample_sub_goal();
        let classification =
            goal.classify("Let me gather the remaining issue events before I can finish.");

        let SubGoalCompletionClassification::Incomplete(message) = classification else {
            panic!("expected incomplete classification")
        };
        assert!(message.contains("next steps instead of completed work"));
    }

    #[test]
    fn sub_goal_without_definition_backfills_task_evidence_terms() {
        let goal = SubGoal::new(
            "Summarize findings",
            Vec::new(),
            SubGoalContract::default(),
            None,
        );

        let contract = goal.contract();
        assert!(contract.require_substantive_text);
        assert!(contract.required_terms.contains(&"summarize".to_string()));
        assert!(contract.required_terms.contains(&"findings".to_string()));

        let SubGoalCompletionClassification::Incomplete(message) = goal.classify("done") else {
            panic!("expected incomplete classification")
        };
        assert!(message.contains("completion evidence markers"));
    }

    #[test]
    fn sub_goal_classification_requires_more_than_one_evidence_marker_when_available() {
        let goal = SubGoal::new(
            "Scaffold the skill",
            Vec::new(),
            SubGoalContract::from_definition_of_done(Some("Scaffolded skill")),
            None,
        );

        let SubGoalCompletionClassification::Incomplete(message) =
            goal.classify("I inspected the skill directory.")
        else {
            panic!("expected incomplete classification")
        };
        assert!(message.contains("matched 1/2"));
    }

    #[test]
    fn action_oriented_sub_goal_rejects_unresolved_blocker_response() {
        let goal = SubGoal::new(
            "Write the x-post spec and scaffold the skill",
            Vec::new(),
            SubGoalContract::from_definition_of_done(Some("Scaffolded skill")),
            None,
        );

        let SubGoalCompletionClassification::Incomplete(message) =
            goal.classify("I tried to scaffold the skill, but the command was not found.")
        else {
            panic!("expected incomplete classification")
        };
        assert!(message.contains("unresolved execution blockers"));
    }

    #[test]
    fn budget_exhausted_outcome_roundtrip_preserves_partial_response() {
        let original = SubGoalOutcome::BudgetExhausted {
            partial_response: Some("researched enough to write the spec".to_string()),
        };

        let encoded = serde_json::to_string(&original).expect("serialize exhausted");
        let decoded: SubGoalOutcome =
            serde_json::from_str(&encoded).expect("deserialize exhausted");

        assert_eq!(decoded, original);
    }
}

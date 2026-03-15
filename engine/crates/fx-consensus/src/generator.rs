use crate::error::ConsensusError;
use crate::orchestrator::CandidateGenerator;
use crate::types::{Candidate, Experiment, NodeId};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationStrategy {
    Conservative,
    Aggressive,
    Creative,
}

pub struct LlmCandidateGenerator {
    node_id: NodeId,
    strategy: GenerationStrategy,
    patch_source: Box<dyn PatchSource>,
}

pub struct PatchResponse {
    pub patch: String,
    pub approach: String,
    pub self_metrics: BTreeMap<String, f64>,
}

#[async_trait]
pub trait PatchSource: Send + Sync {
    async fn generate_patch(
        &self,
        system_prompt: &str,
        experiment: &Experiment,
    ) -> Result<PatchResponse, ConsensusError>;
}

impl LlmCandidateGenerator {
    pub fn new(
        node_id: NodeId,
        strategy: GenerationStrategy,
        patch_source: Box<dyn PatchSource>,
    ) -> Self {
        Self {
            node_id,
            strategy,
            patch_source,
        }
    }

    pub fn strategy(&self) -> &GenerationStrategy {
        &self.strategy
    }

    fn system_prompt(&self, experiment: &Experiment) -> String {
        match self.strategy {
            GenerationStrategy::Conservative => conservative_prompt(experiment),
            GenerationStrategy::Aggressive => aggressive_prompt(experiment),
            GenerationStrategy::Creative => creative_prompt(experiment),
        }
    }
}

#[async_trait]
impl CandidateGenerator for LlmCandidateGenerator {
    async fn generate(&self, experiment: &Experiment) -> Result<Candidate, ConsensusError> {
        let response = self
            .patch_source
            .generate_patch(&self.system_prompt(experiment), experiment)
            .await?;
        Ok(Candidate {
            id: Uuid::new_v4(),
            experiment_id: experiment.id,
            node_id: self.node_id.clone(),
            patch: response.patch,
            approach: response.approach,
            self_metrics: response.self_metrics,
            created_at: Utc::now(),
        })
    }

    fn node_id(&self) -> &NodeId {
        &self.node_id
    }
}

pub fn conservative_prompt(experiment: &Experiment) -> String {
    strategy_prompt(
        "Make the minimal change needed. Do not refactor. Do not change anything outside the immediate fix.",
        experiment,
    )
}

pub fn aggressive_prompt(experiment: &Experiment) -> String {
    strategy_prompt(
        "Fix the root cause. Refactor if it makes the code cleaner. Address related issues you find.",
        experiment,
    )
}

pub fn creative_prompt(experiment: &Experiment) -> String {
    strategy_prompt(
        "Find an unconventional solution. Consider architectural alternatives. The goal is the best possible fix, not the smallest.",
        experiment,
    )
}

fn strategy_prompt(instruction: &str, experiment: &Experiment) -> String {
    let criteria = experiment
        .fitness_criteria
        .iter()
        .map(|criterion| {
            format!(
                "- {} ({:?}, weight={})",
                criterion.name, criterion.metric_type, criterion.weight
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let scope = experiment
        .scope
        .allowed_files
        .iter()
        .map(|path| format!("- {}", path.0))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        concat!(
            "You are generating a unified diff patch for a Proof of Fitness experiment.\n",
            "Strategy: {instruction}\n",
            "NEVER modify kernel/safety paths (Tier 3). These are enforced at runtime but must not be attempted.\n\n",
            "Trigger signal: {signal_name}\n",
            "Signal description: {signal_description}\n",
            "Hypothesis: {hypothesis}\n",
            "Allowed scope (tier {tier:?}):\n{scope}\n\n",
            "Fitness criteria:\n{criteria}\n\n",
            "Return a patch that stays within scope, plus a plain-language approach summary and self-assessed metrics."
        ),
        instruction = instruction,
        signal_name = experiment.trigger.name,
        signal_description = experiment.trigger.description,
        hypothesis = experiment.hypothesis,
        tier = experiment.scope.proposal_tier,
        scope = scope,
        criteria = criteria,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tests::sample_experiment;
    use std::sync::{Arc, Mutex};

    struct RecordingPatchSource {
        prompts: Arc<Mutex<Vec<String>>>,
        response: PatchResponse,
    }

    #[async_trait]
    impl PatchSource for RecordingPatchSource {
        async fn generate_patch(
            &self,
            system_prompt: &str,
            _experiment: &Experiment,
        ) -> Result<PatchResponse, ConsensusError> {
            self.prompts
                .lock()
                .expect("prompt lock")
                .push(system_prompt.to_owned());
            Ok(PatchResponse {
                patch: self.response.patch.clone(),
                approach: self.response.approach.clone(),
                self_metrics: self.response.self_metrics.clone(),
            })
        }
    }

    #[tokio::test]
    async fn strategies_build_distinct_prompts_and_candidates() {
        let experiment = sample_experiment();
        let prompts = Arc::new(Mutex::new(Vec::new()));

        let conservative = build_generator(
            GenerationStrategy::Conservative,
            prompts.clone(),
            "conservative patch",
        );
        let aggressive = build_generator(
            GenerationStrategy::Aggressive,
            prompts.clone(),
            "aggressive patch",
        );
        let creative = build_generator(
            GenerationStrategy::Creative,
            prompts.clone(),
            "creative patch",
        );

        let conservative_candidate = conservative.generate(&experiment).await.expect("generate");
        let aggressive_candidate = aggressive.generate(&experiment).await.expect("generate");
        let creative_candidate = creative.generate(&experiment).await.expect("generate");

        assert_eq!(conservative_candidate.patch, "conservative patch");
        assert_eq!(aggressive_candidate.approach, "aggressive patch");
        assert_eq!(creative_candidate.self_metrics.get("fitness"), Some(&0.9));

        let stored = prompts.lock().expect("prompt lock").clone();
        assert_eq!(stored.len(), 3);
        assert!(stored[0].contains("Make the minimal change needed"));
        assert!(stored[1].contains("Fix the root cause"));
        assert!(stored[2].contains("Find an unconventional solution"));
        assert!(stored
            .iter()
            .all(|prompt| prompt.contains("NEVER modify kernel/safety paths (Tier 3).")));
        assert_ne!(stored[0], stored[1]);
        assert_ne!(stored[1], stored[2]);
        assert_ne!(stored[0], stored[2]);
    }

    fn build_generator(
        strategy: GenerationStrategy,
        prompts: Arc<Mutex<Vec<String>>>,
        text: &str,
    ) -> LlmCandidateGenerator {
        LlmCandidateGenerator::new(
            NodeId::from("node-a"),
            strategy,
            Box::new(RecordingPatchSource {
                prompts,
                response: PatchResponse {
                    patch: text.to_owned(),
                    approach: text.to_owned(),
                    self_metrics: BTreeMap::from([("fitness".into(), 0.9)]),
                },
            }),
        )
    }
}

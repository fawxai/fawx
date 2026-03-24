use fx_consensus::{
    display_strategy, EvaluationWorkspace, Experiment, GenerationStrategy, NeutralEvaluatorConfig,
    NodeConfig, PatchResponse, PatchSource, Signal, StrategyDisplay, TestResult,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn build_nodes(count: u32) -> Vec<NodeConfig> {
    (0..count)
        .map(|index| {
            let strategy = strategy_for(index);
            let node_id = fx_consensus::NodeId(format!("node-{index}"));
            NodeConfig {
                node_id: node_id.clone(),
                strategy: strategy.clone(),
                patch_source: Box::new(PlaceholderPatchSource { strategy, node_id }),
                workspace: Box::new(PlaceholderWorkspace),
            }
        })
        .collect()
}

pub(super) fn build_neutral_evaluator() -> NeutralEvaluatorConfig {
    NeutralEvaluatorConfig {
        node_id: fx_consensus::NodeId("neutral-evaluator".to_owned()),
        workspace: Box::new(PlaceholderWorkspace),
    }
}

pub(super) fn strategy_for(index: u32) -> GenerationStrategy {
    match index % 3 {
        0 => GenerationStrategy::Conservative,
        1 => GenerationStrategy::Aggressive,
        _ => GenerationStrategy::Creative,
    }
}

pub(super) struct PlaceholderPatchSource {
    strategy: GenerationStrategy,
    node_id: fx_consensus::NodeId,
}

#[async_trait::async_trait]
impl PatchSource for PlaceholderPatchSource {
    async fn generate_patch(
        &self,
        _system_prompt: &str,
        experiment: &Experiment,
    ) -> fx_consensus::Result<PatchResponse> {
        Ok(PatchResponse {
            patch: "# Placeholder — wire to LLM in Phase 5".to_owned(),
            approach: placeholder_approach(&self.strategy, experiment),
            self_metrics: placeholder_metrics(&self.node_id, &self.strategy),
        })
    }
}

fn placeholder_approach(strategy: &GenerationStrategy, experiment: &Experiment) -> String {
    let strategy_label: StrategyDisplay<'_> = display_strategy(strategy);
    format!(
        "{strategy_label} strategy placeholder for hypothesis: {}",
        experiment.hypothesis
    )
}

fn placeholder_metrics(
    node_id: &fx_consensus::NodeId,
    strategy: &GenerationStrategy,
) -> BTreeMap<String, f64> {
    BTreeMap::from([
        (
            "build_success".to_owned(),
            deterministic_metric(node_id, strategy, "build_success", 0.70, 0.95),
        ),
        (
            "test_pass_rate".to_owned(),
            deterministic_metric(node_id, strategy, "test_pass_rate", 0.60, 0.98),
        ),
        (
            "signal_resolution".to_owned(),
            deterministic_metric(node_id, strategy, "signal_resolution", 0.25, 0.90),
        ),
    ])
}

fn deterministic_metric(
    node_id: &fx_consensus::NodeId,
    strategy: &GenerationStrategy,
    metric: &str,
    min: f64,
    max: f64,
) -> f64 {
    let seed = format!("{}:{strategy:?}:{metric}", node_id.0);
    let digest = Sha256::digest(seed.as_bytes());
    let value = u64::from_le_bytes(digest[..8].try_into().expect("digest prefix"));
    let ratio = (value as f64) / (u64::MAX as f64);
    min + ((max - min) * ratio)
}

pub(super) struct PlaceholderWorkspace;

#[async_trait::async_trait]
impl EvaluationWorkspace for PlaceholderWorkspace {
    async fn apply_patch(&self, _patch: &str) -> fx_consensus::Result<()> {
        Ok(())
    }

    async fn build(&self) -> fx_consensus::Result<()> {
        Ok(())
    }

    async fn test(&self) -> fx_consensus::Result<TestResult> {
        Ok(TestResult {
            passed: 1,
            failed: 0,
            total: 1,
        })
    }

    async fn check_signal(&self, _signal: &Signal) -> fx_consensus::Result<bool> {
        Ok(false)
    }

    async fn check_regression(&self, _experiment: &Experiment) -> fx_consensus::Result<bool> {
        Ok(false)
    }

    async fn reset(&self) -> fx_consensus::Result<()> {
        Ok(())
    }
}

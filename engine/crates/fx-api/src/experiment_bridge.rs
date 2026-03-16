use crate::experiment_registry::{
    ExperimentConfig, ExperimentKind, ExperimentRegistry, ExperimentResult,
};
use crate::SharedExperimentRegistry;
use fx_tools::ExperimentRegistrar;

const MAX_SCORE_SUMMARY_CHARS: usize = 2_000;

pub struct RegistryBridge {
    registry: SharedExperimentRegistry,
}

impl RegistryBridge {
    pub fn new(registry: SharedExperimentRegistry) -> Self {
        Self { registry }
    }

    fn with_registry<T>(
        &self,
        action: &'static str,
        update: impl FnOnce(&mut ExperimentRegistry) -> Result<T, String>,
    ) -> Option<T> {
        let mut registry = match self.registry.try_lock() {
            Ok(registry) => registry,
            Err(error) => {
                tracing::warn!(action, %error, "experiment registry busy");
                return None;
            }
        };
        match update(&mut registry) {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::warn!(action, %error, "experiment registry update failed");
                None
            }
        }
    }
}

impl ExperimentRegistrar for RegistryBridge {
    fn register_started(&self, signal: &str, hypothesis: &str) -> String {
        let name = format!("{signal}: {hypothesis}");
        self.with_registry("register_started", |registry| {
            let experiment = registry.create(
                name,
                ExperimentKind::ProofOfFitness,
                ExperimentConfig::default(),
            )?;
            registry.start(&experiment.id)?;
            Ok(experiment.id)
        })
        .unwrap_or_default()
    }

    fn register_completed(&self, id: &str, success: bool, summary: &str) {
        if !success {
            self.register_failed(id, "experiment reported unsuccessful completion");
            return;
        }
        let result = completed_result(summary);
        let _ = self.with_registry("register_completed", |registry| {
            registry.complete(id, result)
        });
    }

    fn register_failed(&self, id: &str, error: &str) {
        let message = error.to_string();
        let _ = self.with_registry("register_failed", |registry| registry.fail(id, message));
    }
}

fn completed_result(summary: &str) -> ExperimentResult {
    ExperimentResult {
        plans_generated: 0,
        proposals_written: Vec::new(),
        branches_created: Vec::new(),
        score_summary: normalize_score_summary(summary),
        skipped: Vec::new(),
    }
}

fn normalize_score_summary(summary: &str) -> Option<String> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_SCORE_SUMMARY_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment_registry::{ExperimentRegistry, ExperimentStatus};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn registry_bridge_tracks_background_experiment_lifecycle() {
        let temp = TempDir::new().expect("tempdir");
        let registry = Arc::new(tokio::sync::Mutex::new(
            ExperimentRegistry::new(temp.path()).expect("registry"),
        ));
        let bridge = RegistryBridge::new(Arc::clone(&registry));

        let id = bridge.register_started("latency", "parallelism helps");
        assert!(!id.is_empty());
        assert_eq!(
            experiment_status(&registry, &id),
            Some(ExperimentStatus::Running)
        );

        bridge.register_completed(&id, true, "done");

        let registry = registry.try_lock().expect("registry lock");
        let experiment = registry.get(&id).expect("experiment");
        assert_eq!(experiment.status, ExperimentStatus::Completed);
        assert_eq!(
            experiment
                .result
                .as_ref()
                .expect("result")
                .score_summary
                .as_deref(),
            Some("done")
        );
        assert!(experiment
            .result
            .as_ref()
            .expect("result")
            .skipped
            .is_empty());
    }

    #[test]
    fn completed_result_keeps_up_to_two_thousand_summary_chars() {
        let summary = "x".repeat(MAX_SCORE_SUMMARY_CHARS + 50);
        let result = completed_result(&summary);

        assert_eq!(
            result.score_summary.as_ref().map(String::len),
            Some(MAX_SCORE_SUMMARY_CHARS)
        );
        assert!(result.skipped.is_empty());
    }

    fn experiment_status(
        registry: &SharedExperimentRegistry,
        id: &str,
    ) -> Option<ExperimentStatus> {
        registry
            .try_lock()
            .expect("registry lock")
            .get(id)
            .map(|experiment| experiment.status)
    }
}

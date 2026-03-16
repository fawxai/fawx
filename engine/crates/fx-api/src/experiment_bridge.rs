use crate::experiment_registry::{
    ExperimentConfig, ExperimentKind, ExperimentRegistry, ExperimentResult, SkippedItem,
};
use crate::SharedExperimentRegistry;
use fx_tools::ExperimentRegistrar;

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
        skipped: summary_item(summary),
    }
}

fn summary_item(summary: &str) -> Vec<SkippedItem> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    vec![SkippedItem {
        name: "summary".to_string(),
        reason: trimmed.chars().take(200).collect(),
    }]
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
        assert_eq!(experiment.result.as_ref().expect("result").skipped.len(), 1);
        assert_eq!(
            experiment.result.as_ref().expect("result").skipped[0].name,
            "summary"
        );
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

use crate::{
    CreatedSubagentSession, SpawnConfig, SpawnMode, SubagentControl, SubagentError,
    SubagentFactory, SubagentHandle, SubagentId, SubagentStatus,
};
use async_trait::async_trait;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Reusable benign subagent-control stub for dependent crate tests.
#[derive(Debug)]
pub struct StubSubagentControl {
    state: Mutex<StubSubagentState>,
}

#[derive(Debug)]
struct StubSubagentState {
    label: Option<String>,
    mode: SpawnMode,
    status: SubagentStatus,
    cancel_status: Option<SubagentStatus>,
    reply: String,
    initial_response: Option<String>,
}

impl StubSubagentControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_status(self, status: SubagentStatus) -> Self {
        self.update_state(|state| {
            state.status = status.clone();
            if status != SubagentStatus::Running {
                state.cancel_status = None;
            }
        });
        self
    }

    pub fn with_initial_response(self, initial_response: &str) -> Self {
        self.update_state(|state| {
            state.initial_response = Some(initial_response.to_string());
        });
        self
    }

    fn handle(&self) -> SubagentHandle {
        let state = self.state.lock().expect("stub state lock");
        SubagentHandle {
            id: SubagentId("agent-1".to_string()),
            label: state.label.clone(),
            status: state.status.clone(),
            mode: state.mode,
            started_at: Instant::now(),
            initial_response: state.initial_response.clone(),
        }
    }

    fn update_state(&self, update: impl FnOnce(&mut StubSubagentState)) {
        let mut state = self.state.lock().expect("stub state lock");
        update(&mut state);
    }
}

impl Default for StubSubagentControl {
    fn default() -> Self {
        Self {
            state: Mutex::new(StubSubagentState {
                label: Some("helper".to_string()),
                mode: SpawnMode::Run,
                status: SubagentStatus::Running,
                cancel_status: Some(SubagentStatus::Cancelled),
                reply: "reply".to_string(),
                initial_response: None,
            }),
        }
    }
}

#[async_trait]
impl SubagentControl for StubSubagentControl {
    async fn spawn(&self, _config: SpawnConfig) -> Result<SubagentHandle, SubagentError> {
        Ok(self.handle())
    }

    async fn status(&self, _id: &SubagentId) -> Result<Option<SubagentStatus>, SubagentError> {
        Ok(Some(self.handle().status))
    }

    async fn list(&self) -> Result<Vec<SubagentHandle>, SubagentError> {
        Ok(vec![self.handle()])
    }

    async fn get(&self, id: &SubagentId) -> Result<Option<SubagentHandle>, SubagentError> {
        let handle = self.handle();
        Ok((handle.id == *id).then_some(handle))
    }

    async fn cancel(&self, _id: &SubagentId) -> Result<(), SubagentError> {
        self.update_state(|state| {
            if state.status == SubagentStatus::Running {
                if let Some(status) = state.cancel_status.clone() {
                    state.status = status;
                }
            }
        });
        Ok(())
    }

    async fn send(&self, _id: &SubagentId, _message: &str) -> Result<String, SubagentError> {
        let state = self.state.lock().expect("stub state lock");
        Ok(state.reply.clone())
    }

    async fn gc(&self, _max_age: Duration) -> Result<(), SubagentError> {
        Ok(())
    }
}

/// Reusable disabled factory for tests that should not spawn subagents.
#[derive(Debug, Clone)]
pub struct DisabledSubagentFactory {
    reason: String,
}

impl DisabledSubagentFactory {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Default for DisabledSubagentFactory {
    fn default() -> Self {
        Self::new("subagent spawning is disabled")
    }
}

impl SubagentFactory for DisabledSubagentFactory {
    fn create_session(
        &self,
        _config: &SpawnConfig,
    ) -> Result<CreatedSubagentSession, SubagentError> {
        Err(SubagentError::Spawn(self.reason.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_subagent_control_new_is_benign() {
        let control = StubSubagentControl::new();
        let id = SubagentId("agent-1".to_string());

        let spawned = control
            .spawn(SpawnConfig::new("review"))
            .await
            .expect("spawn");
        let status = control.status(&id).await.expect("status");
        let handles = control.list().await.expect("list");
        let reply = control.send(&id, "ping").await.expect("send");

        control.cancel(&id).await.expect("cancel");
        control.gc(Duration::ZERO).await.expect("gc");

        assert_eq!(spawned.id, id);
        assert_eq!(status, Some(SubagentStatus::Running));
        assert_eq!(handles.len(), 1);
        assert_eq!(reply, "reply");
        assert!(spawned.initial_response.is_none());
    }

    #[tokio::test]
    async fn stub_subagent_control_with_status_updates_returned_status() {
        let control = StubSubagentControl::new().with_status(SubagentStatus::Completed {
            result: "done".to_string(),
            tokens_used: 42,
        });
        let id = SubagentId("agent-1".to_string());
        let status = control.status(&id).await.expect("status");

        assert_eq!(
            status,
            Some(SubagentStatus::Completed {
                result: "done".to_string(),
                tokens_used: 42,
            })
        );
    }

    #[tokio::test]
    async fn stub_subagent_control_with_initial_response_updates_handle() {
        let control = StubSubagentControl::new().with_initial_response("initial");
        let handles = control.list().await.expect("list");

        assert_eq!(handles[0].initial_response.as_deref(), Some("initial"));
    }

    #[test]
    fn disabled_subagent_factory_new_returns_configured_error() {
        let factory = DisabledSubagentFactory::new("disabled");
        let error = factory
            .create_session(&SpawnConfig::new("review"))
            .expect_err("disabled factory should reject spawn");

        assert_eq!(error, SubagentError::Spawn("disabled".to_string()));
    }
}

use crate::instance::spawn_instance;
use crate::{
    SpawnConfig, SubagentControl, SubagentError, SubagentFactory, SubagentHandle, SubagentId,
    SubagentLimits, SubagentStatus,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Dependencies required to build a [`SubagentManager`].
#[derive(Debug)]
pub struct SubagentManagerDeps {
    pub factory: Arc<dyn SubagentFactory>,
    pub limits: SubagentLimits,
}

/// Lifecycle orchestrator for spawned subagents.
#[derive(Debug)]
pub struct SubagentManager {
    factory: Arc<dyn SubagentFactory>,
    limits: SubagentLimits,
    state: Mutex<ManagerState>,
}

#[derive(Debug, Default)]
struct ManagerState {
    instances: HashMap<SubagentId, Arc<crate::instance::SubagentInstance>>,
}

impl SubagentManager {
    /// Create a new in-memory manager.
    pub fn new(deps: SubagentManagerDeps) -> Self {
        Self {
            factory: deps.factory,
            limits: deps.limits,
            state: Mutex::new(ManagerState::default()),
        }
    }

    /// Return a handle snapshot for a specific subagent ID.
    pub async fn snapshot(&self, id: &SubagentId) -> Result<Option<SubagentHandle>, SubagentError> {
        let state = self.state.lock().await;
        Ok(state.instances.get(id).map(|instance| instance.handle()))
    }

    fn prepare_config(&self, mut config: SpawnConfig) -> SpawnConfig {
        if config.timeout.is_zero() {
            config.timeout = self.limits.default_timeout;
        }
        config
    }

    fn spawn_limit_error(&self) -> SubagentError {
        SubagentError::MaxConcurrent(self.limits.max_concurrent)
    }
}

#[async_trait]
impl SubagentControl for SubagentManager {
    async fn spawn(&self, config: SpawnConfig) -> Result<SubagentHandle, SubagentError> {
        let config = self.prepare_config(config);
        let mut state = self.state.lock().await;
        if active_count(&state) >= self.limits.max_concurrent {
            return Err(self.spawn_limit_error());
        }
        let created = self
            .factory
            .create_session(&config)
            .map_err(|error| SubagentError::Spawn(error.to_string()))?;
        let instance = spawn_instance(SubagentId::new(), config, created);
        let handle = instance.handle();
        state.instances.insert(handle.id.clone(), instance);
        Ok(handle)
    }

    async fn status(&self, id: &SubagentId) -> Result<Option<SubagentStatus>, SubagentError> {
        Ok(self.snapshot(id).await?.map(|handle| handle.status))
    }

    async fn list(&self) -> Result<Vec<SubagentHandle>, SubagentError> {
        let state = self.state.lock().await;
        Ok(state
            .instances
            .values()
            .map(|instance| instance.handle())
            .collect())
    }

    async fn get(&self, id: &SubagentId) -> Result<Option<SubagentHandle>, SubagentError> {
        self.snapshot(id).await
    }

    async fn cancel(&self, id: &SubagentId) -> Result<(), SubagentError> {
        let instance = self.instance(id).await?;
        instance.cancel().await;
        Ok(())
    }

    async fn send(&self, id: &SubagentId, message: &str) -> Result<String, SubagentError> {
        let instance = self.instance(id).await?;
        instance.send(message).await
    }

    async fn gc(&self, max_age: Duration) -> Result<(), SubagentError> {
        let ids = self.collect_gc_ids(max_age).await;
        let mut state = self.state.lock().await;
        for id in ids {
            state.instances.remove(&id);
        }
        Ok(())
    }
}

impl SubagentManager {
    async fn instance(
        &self,
        id: &SubagentId,
    ) -> Result<Arc<crate::instance::SubagentInstance>, SubagentError> {
        let state = self.state.lock().await;
        state
            .instances
            .get(id)
            .cloned()
            .ok_or_else(|| SubagentError::NotFound(id.to_string()))
    }

    async fn collect_gc_ids(&self, max_age: Duration) -> Vec<SubagentId> {
        let state = self.state.lock().await;
        state
            .instances
            .iter()
            .filter_map(|(id, instance)| instance.is_gc_eligible(max_age).then_some(id.clone()))
            .collect()
    }
}

fn active_count(state: &ManagerState) -> usize {
    state
        .instances
        .values()
        .map(|instance| instance.handle().status)
        .filter(|status| !status.is_terminal())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreatedSubagentSession, SpawnMode, SubagentSession, SubagentTurn};
    use fx_kernel::cancellation::CancellationToken;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Debug, Clone)]
    struct SessionFactory {
        plans: Arc<Mutex<VecDeque<VecDeque<Step>>>>,
        seen_configs: Arc<Mutex<Vec<SpawnConfig>>>,
    }

    #[derive(Debug)]
    struct MockSession {
        steps: VecDeque<Step>,
        cancel_token: CancellationToken,
    }

    #[derive(Debug, Clone)]
    enum Step {
        Reply {
            response: &'static str,
            tokens_used: u64,
            delay_ms: u64,
        },
        Fail {
            error: &'static str,
        },
        WaitForCancel,
    }

    impl SessionFactory {
        fn new(plans: Vec<VecDeque<Step>>) -> Self {
            Self {
                plans: Arc::new(Mutex::new(VecDeque::from(plans))),
                seen_configs: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn seen_config_count(&self) -> usize {
            self.seen_configs
                .lock()
                .ok()
                .map(|guard| guard.len())
                .unwrap_or_default()
        }
    }

    impl crate::SubagentFactory for SessionFactory {
        fn create_session(
            &self,
            config: &SpawnConfig,
        ) -> Result<CreatedSubagentSession, SubagentError> {
            let token = CancellationToken::new();
            if let Ok(mut guard) = self.seen_configs.lock() {
                guard.push(config.clone());
            }
            let mut plans = self
                .plans
                .lock()
                .map_err(|_| SubagentError::Spawn("factory poisoned".to_string()))?;
            let steps = plans
                .pop_front()
                .ok_or_else(|| SubagentError::Spawn("no session plan available".to_string()))?;
            let session = MockSession {
                steps,
                cancel_token: token.clone(),
            };
            Ok(CreatedSubagentSession {
                session: Box::new(session),
                cancel_token: token,
            })
        }
    }

    #[async_trait]
    impl SubagentSession for MockSession {
        async fn process_message(&mut self, _input: &str) -> Result<SubagentTurn, SubagentError> {
            let step = self
                .steps
                .pop_front()
                .ok_or_else(|| SubagentError::Execution("unexpected message".to_string()))?;
            run_step(step, &self.cancel_token).await
        }
    }

    async fn run_step(
        step: Step,
        cancel_token: &CancellationToken,
    ) -> Result<SubagentTurn, SubagentError> {
        match step {
            Step::Reply {
                response,
                tokens_used,
                delay_ms,
            } => {
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                Ok(SubagentTurn {
                    response: response.to_string(),
                    tokens_used,
                })
            }
            Step::Fail { error } => Err(SubagentError::Execution(error.to_string())),
            Step::WaitForCancel => wait_for_cancel(cancel_token).await,
        }
    }

    async fn wait_for_cancel(
        cancel_token: &CancellationToken,
    ) -> Result<SubagentTurn, SubagentError> {
        while !cancel_token.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(SubagentError::Execution("cancelled".to_string()))
    }

    fn manager_with_plans(plans: Vec<VecDeque<Step>>, limits: SubagentLimits) -> SubagentManager {
        let factory = Arc::new(SessionFactory::new(plans));
        SubagentManager::new(SubagentManagerDeps { factory, limits })
    }

    fn run_config(task: &str) -> SpawnConfig {
        SpawnConfig::new(task)
    }

    fn session_config(task: &str) -> SpawnConfig {
        let mut config = SpawnConfig::new(task);
        config.mode = SpawnMode::Session;
        config
    }

    async fn wait_for_status(
        manager: &SubagentManager,
        id: &SubagentId,
        expected: fn(&SubagentStatus) -> bool,
    ) -> SubagentStatus {
        for _ in 0..50 {
            if let Ok(Some(status)) = manager.status(id).await {
                if expected(&status) {
                    return status;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("status condition not met");
    }

    async fn wait_for_snapshot(
        manager: &SubagentManager,
        id: &SubagentId,
        expected: fn(&SubagentHandle) -> bool,
    ) -> SubagentHandle {
        for _ in 0..50 {
            if let Ok(Some(handle)) = manager.snapshot(id).await {
                if expected(&handle) {
                    return handle;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("handle condition not met");
    }

    #[tokio::test]
    async fn new_manager_starts_empty() {
        let manager = manager_with_plans(Vec::new(), SubagentLimits::default());

        let handles = manager.list().await.expect("list");
        let snapshot = manager
            .snapshot(&SubagentId("missing".to_string()))
            .await
            .expect("snapshot");

        assert!(handles.is_empty());
        assert!(snapshot.is_none());
    }

    #[tokio::test]
    async fn run_mode_subagent_completes_and_stores_result() {
        let plan = VecDeque::from([Step::Reply {
            response: "done",
            tokens_used: 11,
            delay_ms: 0,
        }]);
        let manager = manager_with_plans(vec![plan], SubagentLimits::default());
        let handle = manager.spawn(run_config("review")).await.expect("spawn");

        let status = wait_for_status(&manager, &handle.id, |status| status.is_terminal()).await;

        assert_eq!(
            status,
            SubagentStatus::Completed {
                result: "done".to_string(),
                tokens_used: 11,
            }
        );
    }

    #[tokio::test]
    async fn session_mode_accepts_follow_up_messages() {
        let plan = VecDeque::from([
            Step::Reply {
                response: "initial",
                tokens_used: 3,
                delay_ms: 0,
            },
            Step::Reply {
                response: "follow-up",
                tokens_used: 5,
                delay_ms: 0,
            },
        ]);
        let manager = manager_with_plans(vec![plan], SubagentLimits::default());
        let handle = manager
            .spawn(session_config("debug this"))
            .await
            .expect("spawn");

        assert!(handle.initial_response.is_none());
        let snapshot = wait_for_snapshot(&manager, &handle.id, |snapshot| {
            snapshot.status == SubagentStatus::Running
                && snapshot.initial_response.as_deref() == Some("initial")
        })
        .await;
        let reply = manager
            .send(&handle.id, "try branch b")
            .await
            .expect("send");
        let post_send = manager
            .snapshot(&handle.id)
            .await
            .expect("snapshot")
            .expect("handle");

        assert_eq!(reply, "follow-up");
        assert_eq!(snapshot.initial_response.as_deref(), Some("initial"));
        assert_eq!(snapshot.status, SubagentStatus::Running);
        assert_eq!(post_send.initial_response.as_deref(), Some("initial"));
        assert_eq!(post_send.status, SubagentStatus::Running);
    }

    #[tokio::test]
    async fn max_concurrent_limit_blocks_additional_spawn() {
        let waiting = VecDeque::from([Step::WaitForCancel]);
        let limits = SubagentLimits {
            max_concurrent: 1,
            ..SubagentLimits::default()
        };
        let manager = manager_with_plans(vec![waiting.clone(), waiting], limits);
        let first = manager.spawn(session_config("first")).await.expect("spawn");

        let error = manager
            .spawn(session_config("second"))
            .await
            .expect_err("limit");
        manager.cancel(&first.id).await.expect("cancel");

        assert_eq!(error, SubagentError::MaxConcurrent(1));
    }

    #[tokio::test]
    async fn timeout_marks_subagent_as_timed_out() {
        let plan = VecDeque::from([Step::Reply {
            response: "late",
            tokens_used: 4,
            delay_ms: 50,
        }]);
        let manager = manager_with_plans(vec![plan], SubagentLimits::default());
        let mut config = run_config("slow task");
        config.timeout = Duration::from_millis(10);
        let handle = manager.spawn(config).await.expect("spawn");

        let status = wait_for_status(&manager, &handle.id, |status| {
            *status == SubagentStatus::TimedOut
        })
        .await;

        assert_eq!(status, SubagentStatus::TimedOut);
    }

    #[tokio::test]
    async fn cancel_stops_running_subagent() {
        let waiting = VecDeque::from([Step::WaitForCancel]);
        let manager = manager_with_plans(vec![waiting], SubagentLimits::default());
        let handle = manager
            .spawn(session_config("cancel me"))
            .await
            .expect("spawn");

        manager.cancel(&handle.id).await.expect("cancel");
        let status = wait_for_status(&manager, &handle.id, |status| {
            *status == SubagentStatus::Cancelled
        })
        .await;

        assert_eq!(status, SubagentStatus::Cancelled);
    }

    #[tokio::test]
    async fn gc_removes_completed_instances() {
        let plan = VecDeque::from([Step::Reply {
            response: "done",
            tokens_used: 1,
            delay_ms: 0,
        }]);
        let manager = manager_with_plans(vec![plan], SubagentLimits::default());
        let handle = manager.spawn(run_config("cleanup")).await.expect("spawn");
        let _ = wait_for_status(&manager, &handle.id, |status| status.is_terminal()).await;

        manager.gc(Duration::ZERO).await.expect("gc");
        let handles = manager.list().await.expect("list");

        assert!(handles.is_empty());
    }

    #[tokio::test]
    async fn status_transitions_from_running_to_completed() {
        let plan = VecDeque::from([Step::Reply {
            response: "done",
            tokens_used: 8,
            delay_ms: 20,
        }]);
        let manager = manager_with_plans(vec![plan], SubagentLimits::default());
        let handle = manager
            .spawn(run_config("transition"))
            .await
            .expect("spawn");

        assert_eq!(handle.status, SubagentStatus::Running);
        let status = wait_for_status(&manager, &handle.id, |status| status.is_terminal()).await;

        assert!(status.is_terminal());
    }

    #[tokio::test]
    async fn failed_turn_marks_subagent_as_failed() {
        let plan = VecDeque::from([Step::Fail { error: "boom" }]);
        let manager = manager_with_plans(vec![plan], SubagentLimits::default());
        let handle = manager.spawn(run_config("explode")).await.expect("spawn");

        let status = wait_for_status(&manager, &handle.id, |status| status.is_terminal()).await;

        assert_eq!(
            status,
            SubagentStatus::Failed {
                error: "subagent execution failed: boom".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn spawn_uses_default_timeout_when_config_timeout_is_zero() {
        let factory = Arc::new(SessionFactory::new(vec![VecDeque::from([Step::Reply {
            response: "done",
            tokens_used: 2,
            delay_ms: 0,
        }])]));
        let manager = SubagentManager::new(SubagentManagerDeps {
            factory: Arc::clone(&factory) as Arc<dyn crate::SubagentFactory>,
            limits: SubagentLimits::default(),
        });
        let mut config = run_config("defaults");
        config.timeout = Duration::ZERO;

        let _ = manager.spawn(config).await.expect("spawn");

        assert_eq!(factory.seen_config_count(), 1);
    }
}

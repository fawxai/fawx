use crate::{
    CreatedSubagentSession, SpawnConfig, SpawnMode, SubagentError, SubagentHandle, SubagentId,
    SubagentSession, SubagentStatus, SubagentTurn,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant as TokioInstant;

const CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct SubagentInstance {
    metadata: InstanceMetadata,
    status_tx: watch::Sender<SubagentStatus>,
    status_rx: watch::Receiver<SubagentStatus>,
    command_tx: Mutex<Option<mpsc::Sender<SubagentCommand>>>,
    cancel_token: fx_kernel::cancellation::CancellationToken,
    task_handle: Mutex<Option<JoinHandle<()>>>,
    shared: SharedState,
}

#[derive(Debug, Clone)]
struct InstanceMetadata {
    id: SubagentId,
    label: Option<String>,
    mode: SpawnMode,
    started_at: Instant,
}

#[derive(Debug)]
struct SubagentCommand {
    message: String,
    reply_tx: oneshot::Sender<Result<SubagentTurn, SubagentError>>,
}

#[derive(Debug, Clone)]
struct SharedState {
    finished_at: Arc<Mutex<Option<Instant>>>,
    initial_response: Arc<Mutex<Option<String>>>,
}

pub(crate) fn spawn_instance(
    id: SubagentId,
    config: SpawnConfig,
    created: CreatedSubagentSession,
) -> Arc<SubagentInstance> {
    let metadata = InstanceMetadata::new(id, &config);
    let (status_tx, status_rx) = watch::channel(SubagentStatus::Running);
    let shared = SharedState::new();
    let (command_tx, command_rx) = command_channel(config.mode);
    let cancel_token = created.cancel_token.clone();
    let task = spawn_task(
        config,
        created,
        status_tx.clone(),
        shared.clone(),
        command_rx,
    );

    Arc::new(SubagentInstance {
        metadata,
        status_tx,
        status_rx,
        command_tx: Mutex::new(command_tx),
        cancel_token,
        task_handle: Mutex::new(Some(task)),
        shared,
    })
}

impl SubagentInstance {
    pub(crate) fn handle(&self) -> SubagentHandle {
        SubagentHandle {
            id: self.metadata.id.clone(),
            label: self.metadata.label.clone(),
            status: self.current_status(),
            mode: self.metadata.mode,
            started_at: self.metadata.started_at,
            initial_response: self.shared.initial_response(),
        }
    }

    pub(crate) fn is_gc_eligible(&self, max_age: Duration) -> bool {
        if !self.current_status().is_terminal() {
            return false;
        }
        self.shared
            .finished_at()
            .map(|finished_at| finished_at.elapsed() >= max_age)
            .unwrap_or(false)
    }

    pub(crate) async fn cancel(&self) {
        if self.current_status().is_terminal() {
            return;
        }
        self.cancel_token.cancel();
        self.close_command_channel();
        self.wait_for_shutdown().await;
        self.finish_if_running(SubagentStatus::Cancelled);
    }

    pub(crate) async fn send(&self, message: &str) -> Result<String, SubagentError> {
        let sender = self.command_sender()?;
        let (reply_tx, reply_rx) = oneshot::channel();
        let command = SubagentCommand {
            message: message.to_string(),
            reply_tx,
        };

        sender
            .send(command)
            .await
            .map_err(|_| self.session_closed_error())?;
        let result = reply_rx.await.map_err(|_| self.session_closed_error())?;
        result.map(|turn| turn.response)
    }

    fn command_sender(&self) -> Result<mpsc::Sender<SubagentCommand>, SubagentError> {
        let guard = self
            .command_tx
            .lock()
            .map_err(|_| self.session_closed_error())?;
        guard.clone().ok_or_else(|| self.session_closed_error())
    }

    fn session_closed_error(&self) -> SubagentError {
        SubagentError::SessionClosed(self.metadata.id.to_string())
    }

    fn current_status(&self) -> SubagentStatus {
        self.status_rx.borrow().clone()
    }

    fn close_command_channel(&self) {
        if let Ok(mut guard) = self.command_tx.lock() {
            let _ = guard.take();
        }
    }

    fn take_task_handle(&self) -> Option<JoinHandle<()>> {
        self.task_handle
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
    }

    fn finish_if_running(&self, status: SubagentStatus) {
        if !self.current_status().is_terminal() {
            record_terminal_status(&self.status_tx, &self.shared.finished_at, status);
        }
    }

    async fn wait_for_shutdown(&self) {
        if let Some(handle) = self.take_task_handle() {
            wait_for_task(handle).await;
        }
    }
}

impl InstanceMetadata {
    fn new(id: SubagentId, config: &SpawnConfig) -> Self {
        Self {
            id,
            label: config.label.clone(),
            mode: config.mode,
            started_at: Instant::now(),
        }
    }
}

impl SharedState {
    fn new() -> Self {
        Self {
            finished_at: Arc::new(Mutex::new(None)),
            initial_response: Arc::new(Mutex::new(None)),
        }
    }

    fn finished_at(&self) -> Option<Instant> {
        self.finished_at.lock().ok().and_then(|guard| *guard)
    }

    fn initial_response(&self) -> Option<String> {
        clone_locked_option(&self.initial_response)
    }

    fn store_initial_response(&self, response: String) {
        store_initial_response(&self.initial_response, response);
    }
}

fn command_channel(
    mode: SpawnMode,
) -> (
    Option<mpsc::Sender<SubagentCommand>>,
    Option<mpsc::Receiver<SubagentCommand>>,
) {
    if mode == SpawnMode::Session {
        let (tx, rx) = mpsc::channel(8);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    }
}

fn spawn_task(
    config: SpawnConfig,
    created: CreatedSubagentSession,
    status_tx: watch::Sender<SubagentStatus>,
    shared: SharedState,
    command_rx: Option<mpsc::Receiver<SubagentCommand>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        run_subagent_task(config, created, status_tx, shared, command_rx).await;
    })
}

async fn run_subagent_task(
    config: SpawnConfig,
    created: CreatedSubagentSession,
    status_tx: watch::Sender<SubagentStatus>,
    shared: SharedState,
    command_rx: Option<mpsc::Receiver<SubagentCommand>>,
) {
    let deadline = TokioInstant::now() + config.timeout;
    let mut session = created.session;
    let cancel_token = created.cancel_token;
    let initial = process_turn(&mut *session, &cancel_token, &config.task, deadline).await;

    let Some(tokens_used) =
        handle_initial_turn(&config, initial, &status_tx, &shared, command_rx.is_some())
    else {
        return;
    };

    if let Some(receiver) = command_rx {
        let runtime = SessionRuntime {
            session,
            cancel_token,
            deadline,
            max_tokens: config.max_tokens,
            total_tokens: tokens_used,
            status_tx: &status_tx,
            finished_at: &shared.finished_at,
        };
        run_session_loop(receiver, runtime).await;
    }
}

fn handle_initial_turn(
    config: &SpawnConfig,
    initial: Result<SubagentTurn, SubagentStatus>,
    status_tx: &watch::Sender<SubagentStatus>,
    shared: &SharedState,
    is_session: bool,
) -> Option<u64> {
    match initial {
        Ok(turn) => {
            if let Some(status) =
                budget_status(turn.tokens_used, turn.tokens_used, config.max_tokens)
            {
                record_terminal_status(status_tx, &shared.finished_at, status);
                return None;
            }
            if !is_session {
                let status = SubagentStatus::Completed {
                    result: turn.response,
                    tokens_used: turn.tokens_used,
                };
                record_terminal_status(status_tx, &shared.finished_at, status);
                return None;
            }
            let SubagentTurn {
                response,
                tokens_used,
            } = turn;
            shared.store_initial_response(response);
            Some(tokens_used)
        }
        Err(status) => {
            record_terminal_status(status_tx, &shared.finished_at, status);
            None
        }
    }
}

async fn run_session_loop(
    mut command_rx: mpsc::Receiver<SubagentCommand>,
    mut runtime: SessionRuntime<'_>,
) {
    loop {
        let event = receive_command(&mut command_rx, runtime.deadline).await;
        let Some(command) = event.command else {
            record_terminal_status(runtime.status_tx, runtime.finished_at, event.status);
            return;
        };
        handle_session_command(command, &mut runtime).await;
        if runtime.status_tx.borrow().is_terminal() {
            return;
        }
    }
}

async fn handle_session_command(command: SubagentCommand, runtime: &mut SessionRuntime<'_>) {
    let outcome = process_turn(
        &mut *runtime.session,
        &runtime.cancel_token,
        &command.message,
        runtime.deadline,
    )
    .await;
    let reply = finalize_turn(outcome, &mut runtime.total_tokens, runtime.max_tokens);
    if let Err(status) = &reply {
        record_terminal_status(runtime.status_tx, runtime.finished_at, status.clone());
    }
    let _ = command.reply_tx.send(reply.map_err(error_from_status));
}

async fn receive_command(
    command_rx: &mut mpsc::Receiver<SubagentCommand>,
    deadline: TokioInstant,
) -> SessionEvent {
    match tokio::time::timeout_at(deadline, command_rx.recv()).await {
        Ok(Some(command)) => SessionEvent::command(command),
        Ok(None) => SessionEvent::terminal(SubagentStatus::Cancelled),
        Err(_) => SessionEvent::terminal(SubagentStatus::TimedOut),
    }
}

fn finalize_turn(
    outcome: Result<SubagentTurn, SubagentStatus>,
    total_tokens: &mut u64,
    max_tokens: Option<u64>,
) -> Result<SubagentTurn, SubagentStatus> {
    let turn = outcome?;
    *total_tokens += turn.tokens_used;
    if let Some(status) = budget_status(*total_tokens, turn.tokens_used, max_tokens) {
        return Err(status);
    }
    Ok(turn)
}

async fn process_turn(
    session: &mut dyn SubagentSession,
    cancel_token: &fx_kernel::cancellation::CancellationToken,
    message: &str,
    deadline: TokioInstant,
) -> Result<SubagentTurn, SubagentStatus> {
    if cancel_token.is_cancelled() {
        return Err(SubagentStatus::Cancelled);
    }
    let result = tokio::time::timeout_at(deadline, session.process_message(message)).await;
    map_turn_result(result, cancel_token)
}

fn map_turn_result(
    result: Result<Result<SubagentTurn, SubagentError>, tokio::time::error::Elapsed>,
    cancel_token: &fx_kernel::cancellation::CancellationToken,
) -> Result<SubagentTurn, SubagentStatus> {
    match result {
        Ok(Ok(turn)) => Ok(turn),
        Ok(Err(_)) if cancel_token.is_cancelled() => Err(SubagentStatus::Cancelled),
        Ok(Err(error)) => Err(SubagentStatus::Failed {
            error: error.to_string(),
        }),
        Err(_) => Err(SubagentStatus::TimedOut),
    }
}

fn budget_status(
    total_tokens: u64,
    turn_tokens: u64,
    max_tokens: Option<u64>,
) -> Option<SubagentStatus> {
    let max_tokens = max_tokens?;
    if total_tokens <= max_tokens {
        return None;
    }
    Some(SubagentStatus::Failed {
        error: format!(
            "subagent token budget exceeded: used {total_tokens} tokens (last turn {turn_tokens}) with limit {max_tokens}"
        ),
    })
}

fn error_from_status(status: SubagentStatus) -> SubagentError {
    match status {
        SubagentStatus::Failed { error } => SubagentError::Execution(error),
        SubagentStatus::Cancelled => SubagentError::SessionClosed("subagent cancelled".to_string()),
        SubagentStatus::TimedOut => SubagentError::SessionClosed("subagent timed out".to_string()),
        SubagentStatus::Completed { .. } | SubagentStatus::Running => {
            SubagentError::SessionClosed("subagent is not accepting messages".to_string())
        }
    }
}

fn record_terminal_status(
    status_tx: &watch::Sender<SubagentStatus>,
    finished_at: &Arc<Mutex<Option<Instant>>>,
    status: SubagentStatus,
) {
    let _ = status_tx.send_replace(status);
    if let Ok(mut guard) = finished_at.lock() {
        *guard = Some(Instant::now());
    }
}

fn store_initial_response(initial_response: &Arc<Mutex<Option<String>>>, response: String) {
    match initial_response.lock() {
        Ok(mut guard) => *guard = Some(response),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = Some(response);
        }
    }
}

fn clone_locked_option(initial_response: &Arc<Mutex<Option<String>>>) -> Option<String> {
    match initial_response.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

async fn wait_for_task(mut handle: JoinHandle<()>) {
    match tokio::time::timeout(CANCEL_GRACE_PERIOD, &mut handle).await {
        Ok(result) => {
            let _ = result;
        }
        Err(_) => {
            handle.abort();
            let _ = handle.await;
        }
    }
}

struct SessionEvent {
    command: Option<SubagentCommand>,
    status: SubagentStatus,
}

impl SessionEvent {
    fn command(command: SubagentCommand) -> Self {
        Self {
            command: Some(command),
            status: SubagentStatus::Running,
        }
    }

    fn terminal(status: SubagentStatus) -> Self {
        Self {
            command: None,
            status,
        }
    }
}

struct SessionRuntime<'a> {
    session: Box<dyn SubagentSession>,
    cancel_token: fx_kernel::cancellation::CancellationToken,
    deadline: TokioInstant,
    max_tokens: Option<u64>,
    total_tokens: u64,
    status_tx: &'a watch::Sender<SubagentStatus>,
    finished_at: &'a Arc<Mutex<Option<Instant>>>,
}

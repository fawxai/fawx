use crate::{
    compute_ratios, current_epoch_secs, Canary, CanaryConfig, RollbackReason, RollbackTrigger,
    SignalWindow, Verdict,
};
use fx_kernel::Signal;
use std::sync::Arc;

const DEFAULT_EVAL_INTERVAL: u32 = 10;
const DEFAULT_MIN_BASELINE_CYCLES: u32 = 20;
const DEFAULT_WINDOW_MAX_SIGNALS: usize = 2_000;

/// Manages canary lifecycle: baseline capture, periodic evaluation, and
/// optional rollback triggering.
pub struct CanaryMonitor {
    canary: Canary,
    window: SignalWindow,
    trigger: Option<Arc<dyn RollbackTrigger>>,
    baseline_captured: bool,
    cycles_since_eval: u32,
    eval_interval: u32,
    min_cycles_for_baseline: u32,
    total_cycles: u32,
    min_signals_for_baseline: u64,
    window_seconds: u64,
}

impl CanaryMonitor {
    pub fn new(config: CanaryConfig, trigger: Option<Arc<dyn RollbackTrigger>>) -> Self {
        Self {
            canary: Canary::new(config.clone()),
            window: SignalWindow::new(DEFAULT_WINDOW_MAX_SIGNALS, config.window_seconds),
            trigger,
            baseline_captured: false,
            cycles_since_eval: 0,
            eval_interval: DEFAULT_EVAL_INTERVAL,
            min_cycles_for_baseline: DEFAULT_MIN_BASELINE_CYCLES,
            total_cycles: 0,
            min_signals_for_baseline: config.min_signals_for_baseline,
            window_seconds: config.window_seconds,
        }
    }

    pub fn with_intervals(mut self, eval_interval: u32, min_cycles_for_baseline: u32) -> Self {
        self.eval_interval = eval_interval.max(1);
        self.min_cycles_for_baseline = min_cycles_for_baseline.max(1);
        self
    }

    /// Called after every loop cycle. Ingests signals and evaluates when the
    /// configured interval is reached.
    pub fn on_cycle_complete(&mut self, signals: Vec<Signal>) -> Option<Verdict> {
        self.total_cycles += 1;
        self.cycles_since_eval += 1;
        self.window.ingest(signals);

        if self.capture_baseline_if_ready() {
            return None;
        }

        self.evaluate_if_ready()
    }

    pub fn baseline_captured(&self) -> bool {
        self.baseline_captured
    }

    fn capture_baseline_if_ready(&mut self) -> bool {
        if self.baseline_captured || self.total_cycles < self.min_cycles_for_baseline {
            return false;
        }
        if (self.window.signals().len() as u64) < self.min_signals_for_baseline {
            return false;
        }

        self.canary
            .capture_baseline(self.window.signals(), self.window_seconds);
        self.baseline_captured = true;
        self.cycles_since_eval = 0;
        tracing::info!("canary baseline captured");
        true
    }

    fn evaluate_if_ready(&mut self) -> Option<Verdict> {
        if !self.baseline_captured || self.cycles_since_eval < self.eval_interval {
            return None;
        }

        self.cycles_since_eval = 0;
        let verdict = self.canary.evaluate(self.window.signals());
        self.handle_verdict(&verdict);
        Some(verdict)
    }

    fn handle_verdict(&mut self, verdict: &Verdict) {
        match verdict {
            Verdict::Healthy => {}
            Verdict::Warning { message } => self.log_warning(message),
            Verdict::Degraded {
                message,
                rollback_recommended,
            } => self.handle_degraded(message, *rollback_recommended),
        }
    }

    fn log_warning(&self, message: &str) {
        tracing::warn!(message = %message, "canary warning");
    }

    fn handle_degraded(&mut self, message: &str, rollback_recommended: bool) {
        tracing::error!(
            message = %message,
            rollback_recommended,
            "canary degraded"
        );
        if !rollback_recommended {
            return;
        }
        let Some(reason) = self.rollback_reason(message) else {
            tracing::error!("rollback recommended but no baseline available");
            return;
        };
        self.trigger_rollback(&reason);
    }

    fn rollback_reason(&mut self, message: &str) -> Option<RollbackReason> {
        let baseline = self.canary.baseline()?.clone();
        let current = compute_ratios(self.window.signals());
        Some(RollbackReason {
            verdict_message: message.to_string(),
            current_success_rate: current.success_rate,
            baseline_success_rate: baseline.success_rate,
            timestamp_epoch_secs: current_epoch_secs(),
        })
    }

    fn trigger_rollback(&self, reason: &RollbackReason) {
        let Some(trigger) = &self.trigger else {
            tracing::error!("rollback recommended but ripcord trigger is unavailable");
            return;
        };

        if let Err(error) = trigger.trigger_rollback(reason) {
            tracing::error!(error = %error, "rollback trigger failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_kernel::{LoopStep, SignalKind};
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockTrigger {
        reasons: Mutex<Vec<RollbackReason>>,
    }

    impl RollbackTrigger for MockTrigger {
        fn trigger_rollback(&self, reason: &RollbackReason) -> Result<(), crate::RollbackError> {
            self.reasons
                .lock()
                .expect("lock reasons")
                .push(reason.clone());
            Ok(())
        }
    }

    fn test_config() -> CanaryConfig {
        CanaryConfig {
            min_signals_for_baseline: 1,
            ..CanaryConfig::default()
        }
    }

    fn success_signal() -> Signal {
        mk_signal(SignalKind::Success)
    }

    fn friction_signal() -> Signal {
        mk_signal(SignalKind::Friction)
    }

    fn mk_signal(kind: SignalKind) -> Signal {
        Signal {
            step: LoopStep::Act,
            kind,
            message: String::new(),
            metadata: serde_json::json!({}),
            timestamp_ms: current_epoch_secs() * 1_000,
        }
    }

    #[test]
    fn captures_baseline_after_minimum_cycles() {
        let mut monitor = CanaryMonitor::new(test_config(), None).with_intervals(2, 3);

        assert!(monitor.on_cycle_complete(vec![success_signal()]).is_none());
        assert!(monitor.on_cycle_complete(vec![success_signal()]).is_none());
        assert!(monitor.on_cycle_complete(vec![success_signal()]).is_none());
        assert!(monitor.baseline_captured());
    }

    #[test]
    fn evaluates_after_interval_once_baseline_exists() {
        let mut monitor = CanaryMonitor::new(test_config(), None).with_intervals(2, 1);

        assert!(monitor.on_cycle_complete(vec![success_signal()]).is_none());
        assert!(monitor.on_cycle_complete(vec![success_signal()]).is_none());
        let verdict = monitor.on_cycle_complete(vec![success_signal()]);

        assert_eq!(verdict, Some(Verdict::Healthy));
    }

    #[test]
    fn degraded_verdict_triggers_rollback() {
        let trigger = Arc::new(MockTrigger::default());
        let mut monitor =
            CanaryMonitor::new(test_config(), Some(trigger.clone())).with_intervals(1, 1);

        assert!(monitor
            .on_cycle_complete(vec![success_signal(), success_signal()])
            .is_none());
        let verdict = monitor.on_cycle_complete(vec![friction_signal(), friction_signal()]);

        assert!(matches!(verdict, Some(Verdict::Degraded { .. })));
        assert_eq!(trigger.reasons.lock().expect("lock reasons").len(), 1);
    }

    #[test]
    fn warning_verdict_does_not_trigger_rollback() {
        let trigger = Arc::new(MockTrigger::default());
        let mut monitor =
            CanaryMonitor::new(test_config(), Some(trigger.clone())).with_intervals(1, 1);

        let baseline = std::iter::repeat_with(success_signal)
            .take(20)
            .collect::<Vec<_>>();
        let current = std::iter::repeat_with(success_signal)
            .take(10)
            .chain(std::iter::repeat_with(friction_signal).take(5))
            .collect::<Vec<_>>();

        assert!(monitor.on_cycle_complete(baseline).is_none());
        let verdict = monitor.on_cycle_complete(current);

        assert!(matches!(verdict, Some(Verdict::Warning { .. })));
        assert!(trigger.reasons.lock().expect("lock reasons").is_empty());
    }
}

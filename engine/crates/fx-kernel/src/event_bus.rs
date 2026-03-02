use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// Event published by the kernel when an asynchronous task completes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionEvent {
    /// Stable identifier for the component or task that produced this completion.
    pub source_id: String,
    /// Outcome produced by the completed task.
    pub result: TaskResult,
    /// Milliseconds since epoch when the completion event was emitted.
    pub timestamp_ms: u64,
}

/// Outcome of an asynchronous task reported through the [`EventBus`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskResult {
    /// Task completed successfully with a payload.
    Success(String),
    /// Task failed with an error message payload.
    Failed(String),
    /// Task was cancelled, optionally with a human-readable reason.
    Cancelled(Option<String>),
}

/// Observer for [`CompletionEvent`] notifications emitted by the [`EventBus`].
///
/// # Contract
///
/// Implementors must remain thread-safe and tolerate concurrent invocation.
/// `on_completion` should avoid panicking and return promptly so a slow
/// observer does not delay delivery to others.
pub trait Observer: Send + Sync + fmt::Debug {
    /// Handles a task completion notification from the bus.
    fn on_completion(&self, event: &CompletionEvent);
}

/// Thread-safe registry that broadcasts completion events to observers.
pub struct EventBus {
    observers: Mutex<Vec<(String, Arc<dyn Observer>)>>,
}

impl fmt::Debug for EventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let observers = lock_or_recover(&self.observers);
        let observer_ids: Vec<&str> = observers.iter().map(|(id, _)| id.as_str()).collect();

        f.debug_struct("EventBus")
            .field("observer_count", &observers.len())
            .field("observer_ids", &observer_ids)
            .finish()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Creates an empty event bus with no registered observers.
    pub fn new() -> Self {
        Self {
            observers: Mutex::new(Vec::new()),
        }
    }

    /// Registers an observer under a unique identifier.
    ///
    /// If an observer already exists for `id`, the existing observer is replaced.
    pub fn register(&self, id: &str, observer: Arc<dyn Observer>) {
        let mut observers = lock_or_recover(&self.observers);

        if let Some(index) = observers
            .iter()
            .position(|(observer_id, _)| observer_id == id)
        {
            observers[index] = (id.to_string(), observer);
            return;
        }

        observers.push((id.to_string(), observer));
    }

    /// Removes any observer currently associated with `id`.
    pub fn unregister(&self, id: &str) {
        let mut observers = lock_or_recover(&self.observers);
        observers.retain(|(observer_id, _)| observer_id != id);
    }

    /// Emits `event` to all observers registered at call time.
    ///
    /// The observer list is snapshotted before callbacks run so that
    /// registrations made during delivery only affect subsequent emits.
    pub fn emit(&self, event: CompletionEvent) {
        let observers = {
            let observers = lock_or_recover(&self.observers);
            observers
                .iter()
                .map(|(_, observer)| Arc::clone(observer))
                .collect::<Vec<_>>()
        };

        for observer in observers {
            observer.on_completion(&event);
        }
    }

    /// Returns the number of observers currently registered.
    pub fn observer_count(&self) -> usize {
        let observers = lock_or_recover(&self.observers);
        observers.len()
    }
}

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("Recovered from poisoned EventBus mutex");
            poisoned.into_inner()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    #[derive(Debug)]
    struct MockObserver {
        events: Arc<Mutex<Vec<CompletionEvent>>>,
    }

    impl Observer for MockObserver {
        fn on_completion(&self, event: &CompletionEvent) {
            let mut events = self
                .events
                .lock()
                .expect("test mutex poisoned while recording completion event");
            events.push(event.clone());
        }
    }

    fn sample_event(id: &str, result: TaskResult, timestamp_ms: u64) -> CompletionEvent {
        CompletionEvent {
            source_id: id.to_string(),
            result,
            timestamp_ms,
        }
    }

    fn spawn_emitter(bus: Arc<EventBus>, barrier: Arc<Barrier>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            barrier.wait();
            for index in 0..100 {
                bus.emit(sample_event(
                    &format!("emitter-{index}"),
                    TaskResult::Success("ok".to_string()),
                    index,
                ));
            }
        })
    }

    fn spawn_registrar(bus: Arc<EventBus>, barrier: Arc<Barrier>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            barrier.wait();
            for index in 0..50 {
                let events = Arc::new(Mutex::new(Vec::new()));
                let observer = Arc::new(MockObserver { events });
                bus.register(&format!("dynamic-{index}"), observer);
            }
        })
    }

    fn roundtrip_task_result(task_result: TaskResult) {
        let serialized_result =
            serde_json::to_string(&task_result).expect("task result should serialize to JSON");
        let deserialized_result: TaskResult = serde_json::from_str(&serialized_result)
            .expect("task result should deserialize from JSON");
        assert_eq!(deserialized_result, task_result);
    }

    #[test]
    fn new_creates_empty_bus() {
        let bus = EventBus::new();
        assert_eq!(bus.observer_count(), 0);
    }

    #[test]
    fn register_increments_observer_count() {
        let bus = EventBus::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observer = Arc::new(MockObserver { events });

        bus.register("observer", observer);

        assert_eq!(bus.observer_count(), 1);
    }

    #[test]
    fn register_replaces_existing_observer_with_same_id() {
        let bus = EventBus::new();
        let first_events = Arc::new(Mutex::new(Vec::new()));
        let second_events = Arc::new(Mutex::new(Vec::new()));
        let first_observer = Arc::new(MockObserver {
            events: Arc::clone(&first_events),
        });
        let second_observer = Arc::new(MockObserver {
            events: Arc::clone(&second_events),
        });
        let event = sample_event("task-dup", TaskResult::Success("ok".to_string()), 42);

        bus.register("observer", first_observer);
        bus.register("observer", second_observer);
        bus.emit(event.clone());

        let first = first_events
            .lock()
            .expect("test mutex poisoned while reading first observer events");
        let second = second_events
            .lock()
            .expect("test mutex poisoned while reading second observer events");
        assert!(first.is_empty());
        assert_eq!(second.len(), 1);
        assert_eq!(second[0], event);
        assert_eq!(bus.observer_count(), 1);
    }

    #[test]
    fn emit_calls_registered_observers() {
        let bus = EventBus::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observer = Arc::new(MockObserver {
            events: Arc::clone(&events),
        });
        let event = sample_event("tool:123", TaskResult::Success("ok".to_string()), 123);

        bus.register("observer", observer);
        bus.emit(event.clone());

        let captured = events
            .lock()
            .expect("test mutex poisoned while reading completion events");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0], event);
    }

    #[test]
    fn multiple_observers_receive_the_same_event() {
        let bus = EventBus::new();
        let first_events = Arc::new(Mutex::new(Vec::new()));
        let second_events = Arc::new(Mutex::new(Vec::new()));
        let first_observer = Arc::new(MockObserver {
            events: Arc::clone(&first_events),
        });
        let second_observer = Arc::new(MockObserver {
            events: Arc::clone(&second_events),
        });

        bus.register("first", first_observer);
        bus.register("second", second_observer);
        bus.emit(sample_event(
            "task-A",
            TaskResult::Failed("boom".to_string()),
            55,
        ));

        let first = first_events
            .lock()
            .expect("test mutex poisoned while reading first observer events");
        let second = second_events
            .lock()
            .expect("test mutex poisoned while reading second observer events");
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].source_id, "task-A");
        assert_eq!(second[0].source_id, "task-A");
    }

    #[test]
    fn unregister_removes_observer() {
        let bus = EventBus::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observer = Arc::new(MockObserver {
            events: Arc::clone(&events),
        });

        bus.register("observer", observer);
        bus.unregister("observer");
        bus.emit(sample_event("task-B", TaskResult::Cancelled(None), 77));

        let captured = events
            .lock()
            .expect("test mutex poisoned while reading unregistered observer events");
        assert!(captured.is_empty());
        assert_eq!(bus.observer_count(), 0);
    }

    #[test]
    fn emit_with_no_observers_is_no_op() {
        let bus = EventBus::new();

        bus.emit(sample_event("task-C", TaskResult::Cancelled(None), 1));

        assert_eq!(bus.observer_count(), 0);
    }

    #[test]
    fn supports_concurrent_register_and_emit() {
        let bus = Arc::new(EventBus::new());
        let persistent_events = Arc::new(Mutex::new(Vec::new()));
        let persistent_observer = Arc::new(MockObserver {
            events: Arc::clone(&persistent_events),
        });
        let barrier = Arc::new(Barrier::new(3));

        bus.register("persistent", persistent_observer);

        let emitter_handle = spawn_emitter(Arc::clone(&bus), Arc::clone(&barrier));
        let registrar_handle = spawn_registrar(Arc::clone(&bus), Arc::clone(&barrier));

        barrier.wait();
        emitter_handle
            .join()
            .expect("emitter thread should complete without panicking");
        registrar_handle
            .join()
            .expect("registrar thread should complete without panicking");

        let captured = persistent_events
            .lock()
            .expect("test mutex poisoned while reading concurrent events");
        assert_eq!(captured.len(), 100);
        assert_eq!(bus.observer_count(), 51);
    }

    #[test]
    fn completion_event_and_task_result_serde_roundtrip() {
        let event = sample_event("task-D", TaskResult::Success("done".to_string()), 999);
        let serialized_event =
            serde_json::to_string(&event).expect("event should serialize to JSON");
        let deserialized_event: CompletionEvent =
            serde_json::from_str(&serialized_event).expect("event should deserialize from JSON");

        assert_eq!(deserialized_event, event);
        roundtrip_task_result(TaskResult::Failed("error".to_string()));
        roundtrip_task_result(TaskResult::Cancelled(None));
        roundtrip_task_result(TaskResult::Cancelled(Some("reason".to_string())));
    }
}

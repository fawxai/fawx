use std::sync::{Arc, Mutex};

use crate::streaming::{StreamCallback, StreamEvent};

pub(crate) fn callback_events() -> (StreamCallback, Arc<Mutex<Vec<StreamEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let callback: StreamCallback = Arc::new(move |event| {
        sink.lock().expect("event lock").push(event);
    });
    (callback, events)
}

pub(crate) fn read_events(events: Arc<Mutex<Vec<StreamEvent>>>) -> Vec<StreamEvent> {
    events.lock().expect("event lock").clone()
}

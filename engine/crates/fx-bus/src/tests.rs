use crate::{BusStore, Envelope, Payload, SessionBus};
use fx_session::SessionKey;
use fx_storage::Storage;
use tokio::sync::mpsc::Receiver;
use tokio::time::{timeout, Duration};

fn test_bus() -> (SessionBus, BusStore) {
    let store = BusStore::new(Storage::open_in_memory().expect("in-memory storage"));
    (SessionBus::new(store.clone()), store)
}

fn session_key(value: &str) -> SessionKey {
    SessionKey::new(value).expect("valid session key")
}

fn text_envelope(to: &str, text: &str) -> Envelope {
    Envelope::new(None, session_key(to), Payload::Text(text.to_string()))
}

async fn recv_envelope(receiver: &mut Receiver<Envelope>) -> Envelope {
    timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("message should arrive")
        .expect("channel should stay open")
}

fn drain_count(receiver: &mut Receiver<Envelope>) -> usize {
    let mut count = 0;
    while receiver.try_recv().is_ok() {
        count += 1;
    }
    count
}

#[tokio::test]
async fn send_to_online_session_delivers_immediately() {
    let (bus, store) = test_bus();
    let key = session_key("sess-online");
    let mut receiver = bus.subscribe(&key);

    let result = bus
        .send(text_envelope("sess-online", "hello online"))
        .await
        .expect("send");
    let envelope = recv_envelope(&mut receiver).await;

    assert!(result.delivered);
    assert_eq!(envelope.payload, Payload::Text("hello online".to_string()));
    assert_eq!(store.count(&key).expect("count"), 0);
}

#[tokio::test]
async fn send_to_offline_session_persists_to_store() {
    let (bus, store) = test_bus();
    let key = session_key("sess-offline");

    let result = bus
        .send(text_envelope("sess-offline", "hello offline"))
        .await
        .expect("send");

    assert!(!result.delivered);
    assert_eq!(store.count(&key).expect("count"), 1);
}

#[tokio::test]
async fn subscribe_drains_offline_messages() {
    let (bus, store) = test_bus();
    let key = session_key("sess-drain");
    bus.send(text_envelope("sess-drain", "queued first"))
        .await
        .expect("queue offline message");

    let mut receiver = bus.subscribe(&key);
    let envelope = recv_envelope(&mut receiver).await;

    assert_eq!(envelope.payload, Payload::Text("queued first".to_string()));
    assert_eq!(store.count(&key).expect("count"), 0);
}

#[test]
fn store_read_pending_preserves_messages_until_explicit_delete() {
    let store = BusStore::new(Storage::open_in_memory().expect("in-memory storage"));
    let key = session_key("sess-store-drain");
    let envelope = Envelope::new(None, key.clone(), Payload::Text("hello".to_string()));

    store.enqueue(&envelope).expect("enqueue");
    let pending = store.read_pending(&key).expect("read pending");

    assert_eq!(pending, vec![envelope.clone()]);
    assert_eq!(store.count(&key).expect("count"), 1);
    assert!(store.delete(&key, &envelope.id).expect("delete"));
    assert_eq!(store.count(&key).expect("count after delete"), 0);
}

#[tokio::test]
async fn subscribe_leaves_undelivered_messages_in_store_when_drain_hits_capacity() {
    let (bus, store) = test_bus();
    let key = session_key("sess-capacity");

    for index in 0..300 {
        let text = format!("queued-{index}");
        bus.send(text_envelope("sess-capacity", &text))
            .await
            .expect("queue offline message");
    }

    let mut receiver = bus.subscribe(&key);

    assert_eq!(drain_count(&mut receiver), 256);
    assert_eq!(store.count(&key).expect("count"), 44);

    bus.unsubscribe(&key);
    drop(receiver);

    let mut receiver = bus.subscribe(&key);
    assert_eq!(drain_count(&mut receiver), 44);
    assert_eq!(store.count(&key).expect("count after retry"), 0);
}

#[tokio::test]
async fn unsubscribe_removes_subscriber() {
    let (bus, store) = test_bus();
    let key = session_key("sess-unsub");
    let receiver = bus.subscribe(&key);

    bus.unsubscribe(&key);
    bus.send(text_envelope("sess-unsub", "after unsubscribe"))
        .await
        .expect("send");

    assert!(receiver.is_closed());
    assert_eq!(store.count(&key).expect("count"), 1);
}

#[tokio::test]
async fn multiple_subscribers_independent() {
    let (bus, _store) = test_bus();
    let alpha_key = session_key("sess-alpha");
    let beta_key = session_key("sess-beta");
    let mut alpha = bus.subscribe(&alpha_key);
    let mut beta = bus.subscribe(&beta_key);

    bus.send(text_envelope("sess-alpha", "alpha message"))
        .await
        .expect("send alpha");
    bus.send(text_envelope("sess-beta", "beta message"))
        .await
        .expect("send beta");

    assert_eq!(recv_envelope(&mut alpha).await.to, alpha_key);
    assert_eq!(recv_envelope(&mut beta).await.to, beta_key);
}

#[test]
fn envelope_roundtrip_serde() {
    let envelopes = vec![
        Envelope::new(
            None,
            session_key("sess-1"),
            Payload::Text("hello".to_string()),
        ),
        Envelope::new(
            Some(session_key("sess-parent")),
            session_key("sess-child"),
            Payload::TaskResult {
                task_id: "task-1".to_string(),
                success: true,
                output: "done".to_string(),
            },
        ),
        Envelope::new(
            Some(session_key("sess-worker")),
            session_key("sess-main"),
            Payload::StatusUpdate {
                task_id: "task-2".to_string(),
                progress: "50%".to_string(),
            },
        ),
        Envelope::new(
            None,
            session_key("sess-main"),
            Payload::System("wake".to_string()),
        ),
    ];

    for envelope in envelopes {
        let json = serde_json::to_string(&envelope).expect("serialize envelope");
        let restored: Envelope = serde_json::from_str(&json).expect("deserialize envelope");
        assert_eq!(restored, envelope);
    }
}

#[tokio::test]
async fn channel_full_falls_back_to_store() {
    let (bus, store) = test_bus();
    let key = session_key("sess-full");
    let mut receiver = bus.subscribe(&key);

    for index in 0..256 {
        let text = format!("message-{index}");
        bus.send(text_envelope("sess-full", &text))
            .await
            .expect("fill channel");
    }
    let result = bus
        .send(text_envelope("sess-full", "overflow"))
        .await
        .expect("overflow send");

    assert!(!result.delivered);
    assert_eq!(store.count(&key).expect("count"), 1);
    assert_eq!(drain_count(&mut receiver), 256);
}

#[test]
fn store_delete_removes_specific_message() {
    let store = BusStore::new(Storage::open_in_memory().expect("in-memory storage"));
    let key = session_key("sess-delete");
    let envelope = Envelope::new(None, key.clone(), Payload::Text("hello".to_string()));

    store.enqueue(&envelope).expect("enqueue");
    assert!(store.delete(&key, &envelope.id).expect("delete"));
    assert_eq!(store.count(&key).expect("count"), 0);
}

#[test]
fn store_delete_batch_removes_multiple_messages() {
    let store = BusStore::new(Storage::open_in_memory().expect("in-memory storage"));
    let key = session_key("sess-delete-batch");
    let first = Envelope::new(None, key.clone(), Payload::Text("one".to_string()));
    let second = Envelope::new(None, key.clone(), Payload::Text("two".to_string()));

    store.enqueue(&first).expect("enqueue first");
    store.enqueue(&second).expect("enqueue second");
    store
        .delete_batch(&key, &[first.id.clone(), second.id.clone()])
        .expect("delete batch");

    assert_eq!(store.count(&key).expect("count"), 0);
}

#[tokio::test]
async fn concurrent_send_exercises_back_pressure_and_preserves_messages() {
    let (bus, store) = test_bus();
    let key = session_key("sess-concurrent");
    let mut receiver = bus.subscribe(&key);
    let mut tasks = Vec::new();

    for task_id in 0..10 {
        let bus = bus.clone();
        tasks.push(tokio::spawn(async move {
            for message_id in 0..30 {
                let text = format!("task-{task_id}-message-{message_id}");
                bus.send(text_envelope("sess-concurrent", &text))
                    .await
                    .expect("send message");
            }
        }));
    }

    for task in tasks {
        task.await.expect("task should not panic");
    }

    let first_delivery_count = drain_count(&mut receiver);
    let queued_count = store.count(&key).expect("queued count");

    assert_eq!(first_delivery_count, 256);
    assert_eq!(queued_count, 44);

    bus.unsubscribe(&key);
    drop(receiver);

    let mut receiver = bus.subscribe(&key);
    let replayed_count = drain_count(&mut receiver);

    assert_eq!(first_delivery_count + replayed_count, 300);
    assert_eq!(store.count(&key).expect("final count"), 0);
}

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

pub(crate) async fn spawn_json_server(status_line: &str, body: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local addr");
    let status_line = status_line.to_string();
    let body = body.to_string();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept connection");
        let mut buffer = [0_u8; 2048];
        let _ = socket.read(&mut buffer).await.expect("read request");
        let response = format!(
            "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(), body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    });
    format!("http://{address}")
}

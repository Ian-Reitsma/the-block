use base64::engine::general_purpose;
use base64::Engine;
use futures::SinkExt;
use runtime::net::TcpStream;
use tokio_tungstenite::tungstenite::{protocol::Role, Message};
use tokio_tungstenite::WebSocketStream;

#[cfg(feature = "telemetry")]
use crate::telemetry;
use crate::vm::{vm_debug_enabled, Debugger};

/// Perform WebSocket handshake and stream VM execution trace.
pub async fn serve_vm_trace(mut stream: TcpStream, key: String, code: Vec<u8>) {
    if !vm_debug_enabled() {
        let _ = stream.shutdown().await;
        return;
    }
    let accept_key = {
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(key.as_bytes());
        h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        general_purpose::STANDARD.encode(h.finalize())
    };
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept_key}\r\n\r\n",
    );
    if stream.write_all(resp.as_bytes()).await.is_err() {
        return;
    }
    let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    #[cfg(feature = "telemetry")]
    telemetry::VM_TRACE_TOTAL.inc();
    run_trace(ws_stream, code).await;
}

async fn run_trace(mut ws: WebSocketStream<TcpStream>, code: Vec<u8>) {
    let mut dbg = Debugger::new(code);
    let steps = dbg.run().to_vec();
    for step in steps {
        if ws
            .send(Message::Text(serde_json::to_string(&step).unwrap()))
            .await
            .is_err()
        {
            break;
        }
    }
}

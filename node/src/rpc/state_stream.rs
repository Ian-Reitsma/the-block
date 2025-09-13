use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{protocol::Role, Message};
use tokio_tungstenite::WebSocketStream;

use crate::telemetry;
use crate::Blockchain;

#[derive(Serialize)]
struct DiffChunk {
    seq: u64,
    tip_height: u64,
    accounts: Vec<(String, u64)>,
    root: [u8; 32],
    proof: Vec<u8>,
    compressed: bool,
}

/// Perform a minimal WebSocket handshake and stream state diffs to the client.
pub async fn serve_state_stream(
    mut stream: TcpStream,
    key: String,
    bc: Arc<Mutex<Blockchain>>,
) {
    let accept_key = {
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(key.as_bytes());
        h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        base64::encode(h.finalize())
    };
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept_key}\r\n\r\n"
    );
    if stream.write_all(resp.as_bytes()).await.is_err() {
        return;
    }
    let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    telemetry::STATE_STREAM_SUBSCRIBERS_TOTAL.inc();
    run_stream(ws_stream, bc).await;
}

async fn run_stream(mut ws: WebSocketStream<TcpStream>, bc: Arc<Mutex<Blockchain>>) {
    let mut seq = 0u64;
    loop {
        let tip = { bc.lock().unwrap().chain.last().map(|b| b.index).unwrap_or(0) };
        let chunk = DiffChunk {
            seq,
            tip_height: tip,
            accounts: Vec::new(),
            root: [0u8; 32],
            proof: Vec::new(),
            compressed: false,
        };
        let msg = serde_json::to_string(&chunk).unwrap();
        if ws.send(Message::Text(msg)).await.is_err() {
            break;
        }
        seq += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

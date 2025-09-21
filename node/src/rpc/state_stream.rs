use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose;
use base64::Engine;
use futures::SinkExt;
use light_client::{account_state_value, AccountChunk, StateChunk};
use state::MerkleTrie;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{protocol::Role, Message};
use tokio_tungstenite::WebSocketStream;

#[cfg(feature = "telemetry")]
use crate::telemetry;
use crate::Blockchain;

/// Perform a minimal WebSocket handshake and stream state diffs to the client.
pub async fn serve_state_stream(mut stream: TcpStream, key: String, bc: Arc<Mutex<Blockchain>>) {
    let accept_key = {
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(key.as_bytes());
        h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        general_purpose::STANDARD.encode(h.finalize())
    };
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept_key}\r\n\r\n"
    );
    if stream.write_all(resp.as_bytes()).await.is_err() {
        return;
    }
    let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    #[cfg(feature = "telemetry")]
    telemetry::STATE_STREAM_SUBSCRIBERS_TOTAL.inc();
    run_stream(ws_stream, bc).await;
}

async fn run_stream(mut ws: WebSocketStream<TcpStream>, bc: Arc<Mutex<Blockchain>>) {
    let mut seq = 0u64;
    loop {
        let (tip, accounts) = {
            let guard = bc.lock().unwrap();
            let tip = guard.chain.last().map(|b| b.index).unwrap_or(0);
            let accounts = guard.accounts.clone();
            (tip, accounts)
        };
        let mut trie = MerkleTrie::new();
        for (address, account) in accounts.iter() {
            let value = account_state_value(account.balance.consumer, account.nonce);
            trie.insert(address.as_bytes(), &value);
        }
        let root = trie.root_hash();
        let accounts: Vec<AccountChunk> = accounts
            .iter()
            .map(|(address, account)| AccountChunk {
                address: address.clone(),
                balance: account.balance.consumer,
                account_seq: account.nonce,
                proof: trie
                    .prove(address.as_bytes())
                    .expect("proof exists for inserted account"),
            })
            .collect();
        let chunk = StateChunk {
            seq,
            tip_height: tip,
            accounts,
            root,
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

use std::sync::{Arc, Mutex};
use std::time::Duration;

use light_client::{account_state_value, AccountChunk, StateChunk};
use runtime::net::TcpStream;
use runtime::ws::{self, Message as WsMessage, ServerStream};
use state::MerkleTrie;

#[cfg(feature = "telemetry")]
use crate::telemetry;
use crate::Blockchain;

/// Perform a minimal WebSocket handshake and stream state diffs to the client.
pub async fn serve_state_stream(mut stream: TcpStream, key: String, bc: Arc<Mutex<Blockchain>>) {
    if ws::write_server_handshake(&mut stream, &key, &[])
        .await
        .is_err()
    {
        return;
    }
    let ws_stream = ServerStream::new(stream);
    #[cfg(feature = "telemetry")]
    telemetry::STATE_STREAM_SUBSCRIBERS_TOTAL.inc();
    run_stream(ws_stream, bc).await;
}

async fn run_stream(mut ws: ServerStream, bc: Arc<Mutex<Blockchain>>) {
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
        if ws.send(WsMessage::Text(msg)).await.is_err() {
            break;
        }
        seq += 1;
        runtime::sleep(Duration::from_secs(1)).await;
    }
}

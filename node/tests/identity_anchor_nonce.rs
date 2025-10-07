#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::{ed25519::SigningKey, Signer};
use runtime::{io::read_to_end, net::TcpStream};
use std::convert::TryInto;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tempfile::tempdir;
use the_block::{generate_keypair, rpc::run_rpc_server, transaction::TxDidAnchor, Blockchain};
use util::timeout::expect_timeout;

mod util;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(ref prev) = self.previous {
            std::env::set_var(self.key, prev);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn rpc_request(addr: &str, body: &serde_json::Value) -> serde_json::Value {
    runtime::block_on(async {
        let addr: SocketAddr = addr.parse().expect("valid socket address");
        let mut stream = expect_timeout(TcpStream::connect(addr))
            .await
            .expect("connect to RPC server");
        let payload = serde_json::to_string(body).expect("serialize request");
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            payload.len(),
            payload
        );
        expect_timeout(stream.write_all(req.as_bytes()))
            .await
            .expect("send request");
        let mut resp = Vec::new();
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .expect("read response");
        let resp = String::from_utf8(resp).expect("response is utf8");
        let body_idx = resp.find("\r\n\r\n").expect("headers terminator present");
        serde_json::from_str(&resp[body_idx + 4..]).expect("parse response body")
    })
}

fn anchor_payload(sk: &SigningKey, doc: &str, nonce: u64) -> serde_json::Value {
    let pk_bytes = sk.verifying_key().to_bytes();
    let mut tx = TxDidAnchor {
        address: hex::encode(pk_bytes),
        public_key: pk_bytes.to_vec(),
        document: doc.to_string(),
        nonce,
        signature: Vec::new(),
        remote_attestation: None,
    };
    let sig = sk.sign(tx.owner_digest().as_ref());
    tx.signature = sig.to_bytes().to_vec();
    serde_json::to_value(tx).expect("serialize anchor payload")
}

#[testkit::tb_serial]
fn identity_anchor_nonces_are_scoped_per_address() {
    runtime::block_on(async {
        let dir = tempdir().expect("tempdir");
        let chain_path = dir.path().join("chain");
        let bc = Arc::new(Mutex::new(Blockchain::new(
            chain_path.to_str().expect("chain path"),
        )));
        let mining = Arc::new(AtomicBool::new(false));
        let did_db_path = dir.path().join("did.db");
        let _did_env = EnvVarGuard::set("TB_DID_DB_PATH", did_db_path.to_str().expect("did path"));

        let (tx_ready, rx_ready) = runtime::sync::oneshot::channel();
        let server = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            Default::default(),
            tx_ready,
        ));
        let addr = expect_timeout(rx_ready).await.expect("server ready");

        let (sk1_bytes, _) = generate_keypair();
        let sk1 = SigningKey::from_bytes(&sk1_bytes.try_into().expect("sk1 length"));
        let anchor1 = anchor_payload(&sk1, "{\"id\":1}", 1);
        let addr1_hex = anchor1["address"].as_str().expect("address string");

        let (sk2_bytes, _) = generate_keypair();
        let sk2 = SigningKey::from_bytes(&sk2_bytes.try_into().expect("sk2 length"));
        let anchor2 = anchor_payload(&sk2, "{\"id\":2}", 1);
        let addr2_hex = anchor2["address"].as_str().expect("address string");

        assert_ne!(addr1_hex, addr2_hex, "distinct addresses required");

        let req1 = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "identity.anchor",
            "params": anchor1.clone(),
        });
        let resp1 = rpc_request(&addr, &req1).await;
        assert_eq!(resp1["result"]["address"].as_str(), Some(addr1_hex));
        assert_eq!(resp1["result"]["nonce"].as_u64(), Some(1));
        assert!(resp1.get("error").is_none());

        let req2 = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "identity.anchor",
            "params": anchor2.clone(),
        });
        let resp2 = rpc_request(&addr, &req2).await;
        assert_eq!(resp2["result"]["address"].as_str(), Some(addr2_hex));
        assert_eq!(resp2["result"]["nonce"].as_u64(), Some(1));
        assert!(resp2.get("error").is_none());

        let replay_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "identity.anchor",
            "params": anchor1.clone(),
        });
        let replay = rpc_request(&addr, &replay_req).await;
        assert_eq!(replay["error"]["code"].as_i64(), Some(-32000));
        assert_eq!(replay["error"]["message"].as_str(), Some("replayed nonce"));

        server.abort();
        let _ = server.await;
    });
}

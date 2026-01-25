#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_rpc::{Request as RpcRequest, Response as RpcResponse};
use foundation_serialization::json::Value;
use std::collections::HashSet;
use std::convert::TryInto;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use sys::tempfile::tempdir;
use the_block::identity::{handle_registry::HandleRegistry, DidRegistry};
use the_block::{
    generate_keypair,
    rpc::{fuzz_dispatch_request, fuzz_runtime_config},
    transaction::TxDidAnchor,
    Blockchain,
};

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

fn anchor_payload(sk: &SigningKey, doc: &str, nonce: u64) -> Value {
    let pk_bytes = sk.verifying_key().to_bytes();
    let mut tx = TxDidAnchor {
        address: crypto_suite::hex::encode(pk_bytes),
        public_key: pk_bytes.to_vec(),
        document: doc.to_string(),
        nonce,
        signature: Vec::new(),
        remote_attestation: None,
    };
    let sig = sk.sign(tx.owner_digest().as_ref());
    tx.signature = sig.to_bytes().to_vec();
    foundation_serialization::json::to_value(tx).expect("serialize anchor payload")
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
        let runtime_cfg = fuzz_runtime_config();
        let nonces = Arc::new(Mutex::new(HashSet::new()));
        let handles = Arc::new(Mutex::new(HandleRegistry::open(
            dir.path()
                .join("handles.db")
                .to_str()
                .expect("handles path"),
        )));
        let dids = Arc::new(Mutex::new(DidRegistry::open(&did_db_path)));

        let (sk1_bytes, _) = generate_keypair();
        let sk1 = SigningKey::from_bytes(&sk1_bytes.try_into().expect("sk1 length"));
        let anchor1 = anchor_payload(&sk1, "{\"id\":1}", 1);
        let addr1_hex = anchor1["address"].as_str().expect("address string");

        let (sk2_bytes, _) = generate_keypair();
        let sk2 = SigningKey::from_bytes(&sk2_bytes.try_into().expect("sk2 length"));
        let anchor2 = anchor_payload(&sk2, "{\"id\":2}", 1);
        let addr2_hex = anchor2["address"].as_str().expect("address string");

        assert_ne!(addr1_hex, addr2_hex, "distinct addresses required");

        let req1 = RpcRequest::new("identity.anchor", anchor1.clone()).with_id(1);
        let resp1 = fuzz_dispatch_request(
            Arc::clone(&bc),
            Arc::clone(&mining),
            Arc::clone(&nonces),
            Arc::clone(&handles),
            Arc::clone(&dids),
            Arc::clone(&runtime_cfg),
            None,
            None,
            req1,
            None,
            None,
        );
        match resp1 {
            RpcResponse::Result { result, .. } => {
                assert_eq!(result["address"].as_str(), Some(addr1_hex));
                assert_eq!(result["nonce"].as_u64(), Some(1));
            }
            RpcResponse::Error { error, .. } => panic!("anchor1 error: {:?}", error),
        }

        let req2 = RpcRequest::new("identity.anchor", anchor2.clone()).with_id(2);
        let resp2 = fuzz_dispatch_request(
            Arc::clone(&bc),
            Arc::clone(&mining),
            Arc::clone(&nonces),
            Arc::clone(&handles),
            Arc::clone(&dids),
            Arc::clone(&runtime_cfg),
            None,
            None,
            req2,
            None,
            None,
        );
        match resp2 {
            RpcResponse::Result { result, .. } => {
                assert_eq!(result["address"].as_str(), Some(addr2_hex));
                assert_eq!(result["nonce"].as_u64(), Some(1));
            }
            RpcResponse::Error { error, .. } => panic!("anchor2 error: {:?}", error),
        }

        let replay_req = RpcRequest::new("identity.anchor", anchor1.clone()).with_id(3);
        let replay = fuzz_dispatch_request(
            Arc::clone(&bc),
            Arc::clone(&mining),
            Arc::clone(&nonces),
            Arc::clone(&handles),
            Arc::clone(&dids),
            Arc::clone(&runtime_cfg),
            None,
            None,
            replay_req,
            None,
            None,
        );
        match replay {
            RpcResponse::Error { error, .. } => {
                assert_eq!(error.code, -32000);
                assert_eq!(error.message(), "replayed nonce");
            }
            RpcResponse::Result { result, .. } => {
                panic!("expected replay error, got result {result:?}");
            }
        }
    });
}

use std::collections::HashSet;
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use crypto_suite::signatures::ed25519::SECRET_KEY_LENGTH;
use crypto_suite::transactions::TransactionSigner;
use foundation_fuzz::Unstructured;
use foundation_rpc::Request as RpcRequest;
use foundation_serialization::binary;
use foundation_serialization::json;
use sys::tempfile::{tempdir, TempDir};
use the_block::identity::{handle_registry::HandleRegistry, DidRegistry};
use the_block::rpc::{fuzz_dispatch_request, fuzz_runtime_config};
use the_block::transaction::canonical_payload_bytes;
use the_block::transaction::RawTxPayload;
use the_block::Blockchain;

struct IdentityScratch {
    _handles_dir: TempDir,
    _dids_dir: TempDir,
    handles: Arc<Mutex<HandleRegistry>>,
    dids: Arc<Mutex<DidRegistry>>,
}

impl IdentityScratch {
    fn new() -> Self {
        let handles_dir = tempdir().expect("handles tempdir");
        let handles_path = handles_dir.path().to_path_buf();
        // Ensure the directory exists before opening the registry so the
        // temporary path is ready for SimpleDb initialisation.
        fs::create_dir_all(&handles_path).expect("create handles path");
        let handles_path_str = handles_path.to_string_lossy().into_owned();
        let handles = Arc::new(Mutex::new(HandleRegistry::open(&handles_path_str)));

        let dids_dir = tempdir().expect("dids tempdir");
        let dids_path = dids_dir.path().to_path_buf();
        fs::create_dir_all(&dids_path).expect("create dids path");
        let dids = Arc::new(Mutex::new(DidRegistry::open(&dids_path)));

        Self {
            _handles_dir: handles_dir,
            _dids_dir: dids_dir,
            handles,
            dids,
        }
    }

    fn handles(&self) -> Arc<Mutex<HandleRegistry>> {
        Arc::clone(&self.handles)
    }

    fn dids(&self) -> Arc<Mutex<DidRegistry>> {
        Arc::clone(&self.dids)
    }
}

pub fn run(data: &[u8]) {
    let _ = run_with_response(data);
}

fn dispatch_request(
    request: RpcRequest,
    auth_header: Option<String>,
    peer_ip: Option<IpAddr>,
) -> foundation_rpc::Response {
    let bc = Arc::new(Mutex::new(Blockchain::default()));
    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::new()));
    let identity = IdentityScratch::new();
    let cfg = fuzz_runtime_config();

    fuzz_dispatch_request(
        bc,
        mining,
        nonces,
        identity.handles(),
        identity.dids(),
        cfg,
        None,
        request,
        auth_header,
        peer_ip,
    )
}

pub fn run_with_response(data: &[u8]) -> Option<foundation_rpc::Response> {
    if data.is_empty() {
        return None;
    }

    let (selector, remainder) = match data.split_first() {
        Some((first, rest)) => (*first, rest),
        None => return None,
    };
    let request_len = usize::from(selector).min(remainder.len());
    let (request_bytes, tail) = remainder.split_at(request_len);
    let mut cursor = Unstructured::new(tail);

    let peer_ip = cursor.ip_addr().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let auth = tail
        .split_first()
        .and_then(|(_, rest)| std::str::from_utf8(rest).ok())
        .map(|s| s.trim_matches(char::from(0)).to_string())
        .filter(|s| !s.is_empty());

    let response = if let Ok(request) = RpcRequest::from_slice(request_bytes) {
        Some(dispatch_request(request, auth, Some(peer_ip)))
    } else {
        None
    };

    if let Some(response) = response.as_ref() {
        let _ = json::to_vec(response);
    }

    if data.len() > SECRET_KEY_LENGTH {
        let mut secret = [0u8; SECRET_KEY_LENGTH];
        secret.copy_from_slice(&data[..SECRET_KEY_LENGTH]);
        let payload_bytes = &data[SECRET_KEY_LENGTH..];
        let signer = TransactionSigner::from_chain_id(the_block::constants::CHAIN_ID);
        let (sig, public_key) = signer.sign_with_secret(&secret, payload_bytes);
        let _ = signer.verify_with_public_bytes(&public_key, payload_bytes, &sig);

        // Exercise the canonical serializer with fuzz input to ensure the Python
        // bindings and RPC helpers remain in lock-step with the suite.
        if let Ok(payload) = binary::decode::<RawTxPayload>(payload_bytes) {
            let canonical = canonical_payload_bytes(&payload);
            let _ = signer.sign_with_secret(&secret, &canonical);
        }
    }
    response
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn run_request(request: RpcRequest) -> foundation_rpc::Response {
    dispatch_request(request, None, Some(IpAddr::V4(Ipv4Addr::LOCALHOST)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json::Value;

    #[test]
    fn run_handles_minimal_input() {
        super::run(&[0u8]);
    }

    #[test]
    fn run_with_response_rejects_empty_input() {
        assert!(super::run_with_response(&[]).is_none());
    }

    #[test]
    fn executes_consensus_difficulty_request() {
        let request = RpcRequest::new("consensus.difficulty", Value::Null);
        let response = run_request(request);
        match response {
            foundation_rpc::Response::Result { result, .. } => {
                let difficulty = result
                    .get("difficulty")
                    .and_then(Value::as_u64)
                    .expect("difficulty field present");
                assert_eq!(difficulty, Blockchain::default().difficulty);
            }
            foundation_rpc::Response::Error { error, .. } => {
                panic!("unexpected error response: {error:?}");
            }
        }
    }
}

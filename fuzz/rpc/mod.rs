use std::collections::HashSet;
use std::io::Write;
use std::net::TcpListener;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::thread;

use crypto_suite::signatures::ed25519::SECRET_KEY_LENGTH;
use crypto_suite::transactions::TransactionSigner;
use the_block::identity::DidRegistry;
use the_block::rpc::{fuzz_runtime_config, handle_conn};
use the_block::rpc::identity::handle_registry::HandleRegistry;
use the_block::transaction::canonical_payload_bytes;
use the_block::transaction::RawTxPayload;
use the_block::Blockchain;

pub fn run(data: &[u8]) {
    let bc = Arc::new(Mutex::new(Blockchain::default()));
    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::new()));
    let handles = Arc::new(Mutex::new(HandleRegistry::open("fuzz_handles")));
    let dids = Arc::new(Mutex::new(DidRegistry::open("fuzz_dids")));
    let cfg = fuzz_runtime_config();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let bc_cl = Arc::clone(&bc);
    let mining_cl = Arc::clone(&mining);
    let nonces_cl = Arc::clone(&nonces);
    let handles_cl = Arc::clone(&handles);
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let dids_cl = Arc::clone(&dids);
            runtime::block_on(handle_conn(stream, bc_cl, mining_cl, nonces_cl, handles_cl, dids_cl, cfg));
        }
    });
    if let Ok(mut s) = std::net::TcpStream::connect(addr) {
        let _ = s.write_all(data);
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
        if let Ok(payload) = bincode::deserialize::<RawTxPayload>(payload_bytes) {
            let canonical = canonical_payload_bytes(&payload);
            let _ = signer.sign_with_secret(&secret, &canonical);
        }
    }
}

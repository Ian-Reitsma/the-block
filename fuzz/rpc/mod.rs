use std::collections::HashSet;
use std::io::Write;
use std::net::TcpListener;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::thread;

use the_block::identity::DidRegistry;
use the_block::rpc::{fuzz_runtime_config, handle_conn};
use the_block::rpc::identity::handle_registry::HandleRegistry;
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
            let rt = tokio::runtime::Runtime::new().unwrap();
            let dids_cl = Arc::clone(&dids);
            rt.block_on(handle_conn(stream, bc_cl, mining_cl, nonces_cl, handles_cl, dids_cl, cfg));
        }
    });
    if let Ok(mut s) = std::net::TcpStream::connect(addr) {
        let _ = s.write_all(data);
    }
}

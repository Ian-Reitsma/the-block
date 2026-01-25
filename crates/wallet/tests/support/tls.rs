use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;

use httpd::ServerTlsConfig;
use wallet::Wallet;

use super::{spawn_threaded_httpd, SignerBehavior, SignerState};

pub struct TlsWebSocketSignerMock {
    url: String,
    shutdown: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
}

impl TlsWebSocketSignerMock {
    pub fn new(wallet: Wallet, tls: ServerTlsConfig) -> Self {
        let pk_hex = wallet.public_key_hex();
        let state = SignerState {
            wallet: Arc::new(wallet),
            pk_hex,
            behavior: SignerBehavior::Success,
        };
        let (url, shutdown, thread) = spawn_threaded_httpd(state, "wss", Some(tls));
        TlsWebSocketSignerMock {
            url,
            shutdown,
            _thread: thread,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for TlsWebSocketSignerMock {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Thread will exit on its own when shutdown is set
    }
}

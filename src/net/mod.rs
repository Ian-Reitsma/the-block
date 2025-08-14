mod message;
mod peer;

use crate::{Blockchain, SignedTransaction};
use ed25519_dalek::SigningKey;
use rand_core::{OsRng, RngCore};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

pub use message::{Message, Payload};
pub use peer::PeerSet;

/// A minimal TCP gossip node.
pub struct Node {
    addr: SocketAddr,
    peers: PeerSet,
    chain: Arc<Mutex<Blockchain>>,
    key: SigningKey,
}

impl Node {
    /// Create a new node bound to `addr` and seeded with `peers`.
    pub fn new(addr: SocketAddr, peers: Vec<SocketAddr>, bc: Blockchain) -> Self {
        let key = load_net_key();
        Self {
            addr,
            peers: PeerSet::new(peers),
            chain: Arc::new(Mutex::new(bc)),
            key,
        }
    }

    /// Start the listener thread handling inbound gossip.
    pub fn start(&self) -> thread::JoinHandle<()> {
        let listener = TcpListener::bind(self.addr).unwrap_or_else(|e| panic!("bind: {e}"));
        let peers = self.peers.clone();
        let chain = Arc::clone(&self.chain);
        thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(mut stream) = stream {
                    let mut buf = Vec::new();
                    if stream.read_to_end(&mut buf).is_ok() {
                        if let Ok(msg) = bincode::deserialize::<Message>(&buf) {
                            peers.handle_message(msg, &chain);
                        }
                    }
                }
            }
        })
    }

    /// Broadcast a transaction to all known peers.
    pub fn broadcast_tx(&self, tx: SignedTransaction) {
        self.broadcast_payload(Payload::Tx(tx));
    }

    /// Broadcast the current chain to all known peers.
    pub fn broadcast_chain(&self) {
        if let Ok(bc) = self.chain.lock() {
            self.broadcast_payload(Payload::Chain(bc.chain.clone()));
        }
    }

    /// Send a hello message advertising peers.
    pub fn hello(&self) {
        let mut addrs = self.peers.list();
        addrs.push(self.addr);
        self.broadcast_payload(Payload::Hello(addrs));
    }

    /// Access the underlying blockchain.
    pub fn blockchain(&self) -> std::sync::MutexGuard<'_, Blockchain> {
        self.chain.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn broadcast_payload(&self, body: Payload) {
        let msg = Message::new(body, &self.key);
        self.broadcast(&msg);
    }

    fn broadcast(&self, msg: &Message) {
        for peer in self.peers.list() {
            let _ = send_msg(peer, msg);
        }
    }
}

pub(crate) fn send_msg(addr: SocketAddr, msg: &Message) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    let bytes = bincode::serialize(msg).unwrap_or_else(|e| panic!("serialize: {e}"));
    stream.write_all(&bytes)?;
    Ok(())
}

pub(crate) fn load_net_key() -> SigningKey {
    if let Ok(bytes) = fs::read("net_key") {
        if bytes.len() == 64 {
            let mut arr = [0u8; 64];
            arr.copy_from_slice(&bytes);
            if let Ok(sk) = SigningKey::from_keypair_bytes(&arr) {
                return sk;
            }
        }
    }
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    fs::write("net_key", sk.to_keypair_bytes()).unwrap_or_else(|e| panic!("write net_key: {e}"));
    sk
}

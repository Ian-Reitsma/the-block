pub mod ban_store;
mod message;
mod peer;

use crate::{Blockchain, ShutdownFlag, SignedTransaction};
use ed25519_dalek::SigningKey;
use rand_core::{OsRng, RngCore};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{atomic::Ordering, Arc, Mutex};
use std::thread;
use std::time::Duration;

pub use message::{Handshake, Message, Payload};
pub use peer::PeerSet;

/// Current gossip protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Feature bits required for peer connections.
pub const COMPUTE_MARKET_V1: u32 = crate::p2p::FeatureBit::ComputeMarketV1 as u32;
pub const REQUIRED_FEATURES: u32 =
    (crate::p2p::FeatureBit::FeeRoutingV2 as u32) | COMPUTE_MARKET_V1;

/// Feature bits this node advertises.
pub const LOCAL_FEATURES: u32 = REQUIRED_FEATURES;

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
        ban_store::store()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .purge_expired();
        Self {
            addr,
            peers: PeerSet::new(peers),
            chain: Arc::new(Mutex::new(bc)),
            key,
        }
    }

    /// Start the listener thread handling inbound gossip.
    pub fn start(&self) -> thread::JoinHandle<()> {
        let flag = ShutdownFlag::new();
        self.start_with_flag(&flag)
    }

    /// Start the listener thread handling inbound gossip that stops when `shutdown` is triggered.
    pub fn start_with_flag(&self, shutdown: &ShutdownFlag) -> thread::JoinHandle<()> {
        let listener = TcpListener::bind(self.addr).unwrap_or_else(|e| panic!("bind: {e}"));
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|e| panic!("nonblock: {e}"));
        let stop = shutdown.as_arc();
        let peers = self.peers.clone();
        let chain = Arc::clone(&self.chain);
        thread::spawn(move || loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    let addr = Some(addr);
                    let mut buf = Vec::new();
                    if stream.read_to_end(&mut buf).is_ok() {
                        if let Ok(msg) = bincode::deserialize::<Message>(&buf) {
                            peers.handle_message(msg, addr, &chain);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
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

    /// Perform peer discovery by handshaking with known peers and exchanging address lists.
    pub fn discover_peers(&self) {
        let peers = self.peers.list();
        // send handshake to each peer
        let hs = Handshake {
            node_id: self.key.verifying_key().to_bytes(),
            protocol_version: PROTOCOL_VERSION,
            features: LOCAL_FEATURES,
        };
        let hs_msg = Message::new(Payload::Handshake(hs), &self.key);
        for p in &peers {
            let _ = send_msg(*p, &hs_msg);
        }
        // advertise our peer set
        let mut addrs = peers.clone();
        addrs.push(self.addr);
        let hello_msg = Message::new(Payload::Hello(addrs), &self.key);
        for p in self.peers.list() {
            let _ = send_msg(p, &hello_msg);
        }
    }

    /// Snapshot known peer addresses.
    pub fn peer_addrs(&self) -> Vec<SocketAddr> {
        self.peers.list()
    }

    /// Add a peer address to this node.
    pub fn add_peer(&self, addr: SocketAddr) {
        self.peers.add(addr);
    }

    /// Remove a peer address from this node.
    pub fn remove_peer(&self, addr: SocketAddr) {
        self.peers.remove(addr);
    }

    /// Clear all peers from this node.
    pub fn clear_peers(&self) {
        self.peers.clear();
    }

    /// Return this node's listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Load seed peer addresses from `config` and perform discovery.
    pub fn discover_peers_from_file<P: AsRef<std::path::Path>>(&self, config: P) {
        if let Ok(data) = fs::read_to_string(config) {
            for line in data.lines() {
                if let Ok(addr) = line.trim().parse::<SocketAddr>() {
                    self.peers.add(addr);
                }
            }
        }
        self.discover_peers();
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
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(1))?;
    let bytes = bincode::serialize(msg).unwrap_or_else(|e| panic!("serialize: {e}"));
    stream.write_all(&bytes)?;
    Ok(())
}

pub(crate) fn load_net_key() -> SigningKey {
    let path = std::env::var("TB_NET_KEY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("net_key")
        });
    if let Ok(bytes) = fs::read(&path) {
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
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, sk.to_keypair_bytes()).unwrap_or_else(|e| panic!("write net_key: {e}"));
    sk
}

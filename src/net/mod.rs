mod message;
mod peer;

use crate::{Blockchain, SignedTransaction};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

pub use message::Message;
pub use peer::PeerSet;

/// A minimal TCP gossip node.
pub struct Node {
    addr: SocketAddr,
    peers: PeerSet,
    chain: Arc<Mutex<Blockchain>>,
}

impl Node {
    /// Create a new node bound to `addr` and seeded with `peers`.
    pub fn new(addr: SocketAddr, peers: Vec<SocketAddr>, bc: Blockchain) -> Self {
        Self {
            addr,
            peers: PeerSet::new(peers),
            chain: Arc::new(Mutex::new(bc)),
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
                            match msg {
                                Message::Hello(addrs) => {
                                    for a in addrs {
                                        peers.add(a);
                                    }
                                }
                                Message::Tx(tx) => {
                                    if let Ok(mut bc) = chain.lock() {
                                        let _ = bc.submit_transaction(tx);
                                    }
                                }
                                Message::Block(block) => {
                                    if let Ok(mut bc) = chain.lock() {
                                        if (block.index as usize) == bc.chain.len() {
                                            let prev = bc
                                                .chain
                                                .last()
                                                .map(|b| b.hash.clone())
                                                .unwrap_or_default();
                                            if block.index == 0 || block.previous_hash == prev {
                                                let mut new_chain = bc.chain.clone();
                                                new_chain.push(block.clone());
                                                if bc.import_chain(new_chain.clone()).is_ok() {
                                                    drop(bc);
                                                    let msg = Message::Chain(new_chain);
                                                    for p in peers.list() {
                                                        let _ = send_msg(p, &msg);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Message::Chain(new_chain) => {
                                    if let Ok(mut bc) = chain.lock() {
                                        if new_chain.len() > bc.chain.len() {
                                            let _ = bc.import_chain(new_chain);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    /// Broadcast a transaction to all known peers.
    pub fn broadcast_tx(&self, tx: SignedTransaction) {
        self.broadcast(&Message::Tx(tx));
    }

    /// Broadcast the current chain to all known peers.
    pub fn broadcast_chain(&self) {
        if let Ok(bc) = self.chain.lock() {
            self.broadcast(&Message::Chain(bc.chain.clone()));
        }
    }

    /// Send a hello message advertising peers.
    pub fn hello(&self) {
        let mut addrs = self.peers.list();
        addrs.push(self.addr);
        self.broadcast(&Message::Hello(addrs));
    }

    /// Access the underlying blockchain.
    pub fn blockchain(&self) -> std::sync::MutexGuard<'_, Blockchain> {
        self.chain.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn broadcast(&self, msg: &Message) {
        for peer in self.peers.list() {
            let _ = send_msg(peer, msg);
        }
    }
}

fn send_msg(addr: SocketAddr, msg: &Message) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    let bytes = bincode::serialize(msg).unwrap_or_else(|e| panic!("serialize: {e}"));
    stream.write_all(&bytes)?;
    Ok(())
}

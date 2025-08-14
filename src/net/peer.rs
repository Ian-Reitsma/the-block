use super::{load_net_key, send_msg};
use crate::net::message::{Message, Payload};
use crate::Blockchain;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

/// Thread-safe peer set used by the gossip layer.
#[derive(Clone, Default)]
pub struct PeerSet {
    addrs: Arc<Mutex<HashSet<SocketAddr>>>,
    authorized: Arc<Mutex<HashSet<[u8; 32]>>>,
}

impl PeerSet {
    /// Create a new set seeded with `initial` peers.
    pub fn new(initial: Vec<SocketAddr>) -> Self {
        let set: HashSet<_> = initial.into_iter().collect();
        Self {
            addrs: Arc::new(Mutex::new(set)),
            authorized: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Add a peer to the set.
    pub fn add(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.addrs.lock() {
            guard.insert(addr);
        }
    }

    /// Return a snapshot of known peers.
    pub fn list(&self) -> Vec<SocketAddr> {
        self.addrs
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default()
    }

    fn authorize(&self, pk: [u8; 32]) {
        if let Ok(mut set) = self.authorized.lock() {
            set.insert(pk);
        }
    }

    fn is_authorized(&self, pk: &[u8; 32]) -> bool {
        self.authorized
            .lock()
            .map(|s| s.contains(pk))
            .unwrap_or(false)
    }

    /// Verify and handle an incoming message. Unknown peers or bad signatures are dropped.
    pub fn handle_message(&self, msg: Message, chain: &Arc<Mutex<Blockchain>>) {
        let bytes = match bincode::serialize(&msg.body) {
            Ok(b) => b,
            Err(_) => return,
        };
        let pk = match VerifyingKey::from_bytes(&msg.pubkey) {
            Ok(p) => p,
            Err(_) => return,
        };
        let sig = match Signature::from_slice(&msg.signature) {
            Ok(s) => s,
            Err(_) => return,
        };
        if pk.verify(&bytes, &sig).is_err() {
            return;
        }

        match msg.body {
            Payload::Hello(addrs) => {
                self.authorize(msg.pubkey);
                for a in addrs {
                    self.add(a);
                }
            }
            Payload::Tx(tx) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Ok(mut bc) = chain.lock() {
                    let _ = bc.submit_transaction(tx);
                }
            }
            Payload::Block(block) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Ok(mut bc) = chain.lock() {
                    if (block.index as usize) == bc.chain.len() {
                        let prev = bc.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                        if block.index == 0 || block.previous_hash == prev {
                            let mut new_chain = bc.chain.clone();
                            new_chain.push(block.clone());
                            if bc.import_chain(new_chain.clone()).is_ok() {
                                drop(bc);
                                let msg = Message::new(Payload::Chain(new_chain), &load_net_key());
                                for p in self.list() {
                                    let _ = send_msg(p, &msg);
                                }
                                return;
                            }
                        }
                    }
                }
            }
            Payload::Chain(new_chain) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Ok(mut bc) = chain.lock() {
                    if new_chain.len() > bc.chain.len() {
                        let _ = bc.import_chain(new_chain);
                    }
                }
            }
        }
    }
}

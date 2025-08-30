use libp2p::{
    kad::{store::MemoryStore, Behaviour as KadBehaviour, Config as KadConfig},
    Multiaddr, PeerId,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    peers: Vec<(Vec<u8>, Vec<u8>)>,
}

pub struct Discovery {
    kademlia: KadBehaviour<MemoryStore>,
    db_path: PathBuf,
    peers: HashMap<PeerId, Multiaddr>,
}

impl Discovery {
    pub fn new(local: PeerId, path: &str) -> Self {
        let cfg = KadConfig::default();
        let store = MemoryStore::new(local);
        let kademlia = KadBehaviour::with_config(local, store, cfg);
        let db_path = PathBuf::from(path);
        let mut disc = Self {
            kademlia,
            db_path: db_path.clone(),
            peers: HashMap::new(),
        };
        disc.load();
        disc
    }

    fn load(&mut self) {
        if let Ok(bytes) = fs::read(&self.db_path) {
            if let Ok(p) = bincode::deserialize::<Persisted>(&bytes) {
                for (pid, addr_bytes) in p.peers {
                    if let Ok(peer) = PeerId::from_bytes(&pid) {
                        if let Ok(addr) = Multiaddr::try_from(addr_bytes) {
                            self.kademlia.add_address(&peer, addr.clone());
                            self.peers.insert(peer, addr);
                        }
                    }
                }
            }
        }
    }

    pub fn add_peer(&mut self, peer: PeerId, addr: Multiaddr) {
        if self.peers.insert(peer, addr.clone()).is_none() {
            self.kademlia.add_address(&peer, addr);
        }
    }

    pub fn persist(&self) {
        let list: Vec<(Vec<u8>, Vec<u8>)> = self
            .peers
            .iter()
            .map(|(p, a)| (p.to_bytes(), a.to_vec()))
            .collect();
        let bytes = bincode::serialize(&Persisted { peers: list }).unwrap();
        let _ = fs::write(&self.db_path, bytes);
    }

    pub fn has_peer(&self, peer: &PeerId) -> bool {
        self.peers.contains_key(peer)
    }
}

use crate::trie::MerkleTrie;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SnapshotError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Bincode(#[from] Box<bincode::ErrorKind>),
}

/// Serializable snapshot of the trie.
#[derive(Serialize, Deserialize, Clone)]
pub struct Snapshot {
    pub root: [u8; 32],
    pub entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Snapshot {
    pub fn from_trie(trie: &MerkleTrie) -> Self {
        Snapshot {
            root: trie.root_hash(),
            entries: trie
                .map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    pub fn to_trie(self) -> MerkleTrie {
        let mut trie = MerkleTrie::new();
        for (k, v) in self.entries {
            trie.insert(&k, &v);
        }
        trie
    }
}

/// Manager responsible for periodic snapshotting and pruning.
pub struct SnapshotManager {
    dir: PathBuf,
    keep: usize,
}

impl SnapshotManager {
    pub fn new(dir: PathBuf, keep: usize) -> Self {
        Self { dir, keep }
    }

    pub fn snapshot(&self, trie: &MerkleTrie) -> Result<PathBuf, SnapshotError> {
        fs::create_dir_all(&self.dir)?;
        let snap = Snapshot::from_trie(trie);
        let path = self.dir.join(format!("{}.bin", hex::encode(snap.root)));
        let mut file = File::create(&path)?;
        let bytes = bincode::serialize(&snap)?;
        file.write_all(&bytes)?;
        self.prune()?;
        Ok(path)
    }

    pub fn restore(&self, path: &Path) -> Result<MerkleTrie, SnapshotError> {
        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let snap: Snapshot = bincode::deserialize(&buf)?;
        Ok(snap.to_trie())
    }

    fn prune(&self) -> Result<(), SnapshotError> {
        let mut entries: Vec<_> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        while entries.len() > self.keep {
            if let Some(path) = entries.first() {
                let _ = fs::remove_file(path);
            }
            entries.remove(0);
        }
        Ok(())
    }
}

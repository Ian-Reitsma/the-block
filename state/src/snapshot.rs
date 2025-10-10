use crate::{audit, trie::MerkleTrie};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SnapshotError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt snapshot: {0}")]
    Corrupt(String),
}

/// Serializable snapshot of the trie.
pub struct Snapshot {
    pub root: [u8; 32],
    pub entries: Vec<(Vec<u8>, Vec<u8>)>,
    pub engine_backend: Option<String>,
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
            engine_backend: None,
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
    engine_backend: Option<String>,
}

impl SnapshotManager {
    pub fn new(dir: PathBuf, keep: usize) -> Self {
        Self::new_with_engine(dir, keep, None)
    }

    pub fn new_with_engine(dir: PathBuf, keep: usize, engine_backend: Option<String>) -> Self {
        Self {
            dir,
            keep,
            engine_backend,
        }
    }

    pub fn snapshot(&self, trie: &MerkleTrie) -> Result<PathBuf, SnapshotError> {
        fs::create_dir_all(&self.dir)?;
        let mut snap = Snapshot::from_trie(trie);
        if snap.engine_backend.is_none() {
            snap.engine_backend = self.engine_backend.clone();
        }
        let path = self
            .dir
            .join(format!("{}.bin", crypto_suite::hex::encode(snap.root)));
        let mut file = File::create(&path)?;
        let bytes = snap.encode();
        file.write_all(&bytes)?;
        self.prune()?;
        Ok(path)
    }

    pub fn restore(&self, path: &Path) -> Result<MerkleTrie, SnapshotError> {
        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let snap = Snapshot::decode(&buf)?;
        if let (Some(created), Some(target)) =
            (snap.engine_backend.as_ref(), self.engine_backend.as_ref())
        {
            if created != target {
                self.record_engine_migration(created, target)?;
            }
        }
        Ok(snap.to_trie())
    }

    fn record_engine_migration(&self, from: &str, to: &str) -> Result<(), SnapshotError> {
        let path = self.dir.join("engine_migrations.log");
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "migrated snapshot engine from {from} to {to}")?;
        audit::append_engine_migration(&self.dir, from, to)?;
        Ok(())
    }

    fn prune(&self) -> Result<(), SnapshotError> {
        let read_dir = match fs::read_dir(&self.dir) {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };

        let mut entries = Vec::new();
        for res in read_dir {
            let entry = res?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().or_else(|_| metadata.created())?;
            let timestamp = modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO);
            entries.push((entry.path(), timestamp));
        }

        // Sort newest-to-oldest using filesystem modification times, falling back to the
        // filename when timestamps collide so pruning remains deterministic.
        entries.sort_by(|(path_a, time_a), (path_b, time_b)| {
            time_b.cmp(time_a).then_with(|| path_a.cmp(path_b))
        });

        for (idx, (path, _)) in entries.iter().enumerate() {
            if idx >= self.keep {
                fs::remove_file(path)?;
            }
        }

        Ok(())
    }
}

impl Snapshot {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.root);
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        for (key, value) in &self.entries {
            out.extend_from_slice(&(key.len() as u32).to_be_bytes());
            out.extend_from_slice(key);
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
            out.extend_from_slice(value);
        }
        match &self.engine_backend {
            Some(engine) => {
                out.push(1);
                let bytes = engine.as_bytes();
                out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                out.extend_from_slice(bytes);
            }
            None => out.push(0),
        }
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let mut cursor = 0usize;
        if bytes.len() < 32 {
            return Err(SnapshotError::Corrupt("missing root".into()));
        }
        let mut root = [0u8; 32];
        root.copy_from_slice(&bytes[..32]);
        cursor += 32;

        let entry_count = read_u32(bytes, &mut cursor, "missing entry count")? as usize;
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let key_len = read_u32(bytes, &mut cursor, "missing key length")? as usize;
            let key = read_bytes(bytes, &mut cursor, key_len, "truncated key")?.to_vec();
            let value_len = read_u32(bytes, &mut cursor, "missing value length")? as usize;
            let value = read_bytes(bytes, &mut cursor, value_len, "truncated value")?.to_vec();
            entries.push((key, value));
        }

        let engine_backend = if cursor >= bytes.len() {
            None
        } else {
            let flag = bytes[cursor];
            cursor += 1;
            match flag {
                0 => None,
                1 => {
                    let len = read_u32(bytes, &mut cursor, "missing engine length")? as usize;
                    let raw = read_bytes(bytes, &mut cursor, len, "truncated engine label")?;
                    Some(
                        String::from_utf8(raw.to_vec())
                            .map_err(|_| SnapshotError::Corrupt("invalid engine utf8".into()))?,
                    )
                }
                _ => return Err(SnapshotError::Corrupt("invalid engine flag".into())),
            }
        };

        Ok(Snapshot {
            root,
            entries,
            engine_backend,
        })
    }
}

fn read_u32(bytes: &[u8], cursor: &mut usize, message: &str) -> Result<u32, SnapshotError> {
    if bytes.len().saturating_sub(*cursor) < 4 {
        return Err(SnapshotError::Corrupt(message.into()));
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&bytes[*cursor..*cursor + 4]);
    *cursor += 4;
    Ok(u32::from_be_bytes(buf))
}

fn read_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    message: &str,
) -> Result<&'a [u8], SnapshotError> {
    if bytes.len().saturating_sub(*cursor) < len {
        return Err(SnapshotError::Corrupt(message.into()));
    }
    let slice = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice)
}

#[cfg(feature = "telemetry")]
use crate::telemetry::{SNAPSHOT_DURATION_SECONDS, SNAPSHOT_FAIL_TOTAL};
use crate::{Account, TokenBalance};
use hex;
use state::MerkleTrie;
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::fs;
use std::path::Path;

#[derive(Clone)]
pub struct SnapshotManager {
    pub base: String,
    pub interval: u64,
}

impl SnapshotManager {
    pub fn new(base: String, interval: u64) -> Self {
        Self { base, interval }
    }
    pub fn set_base(&mut self, base: String) {
        self.base = base;
    }
    pub fn set_interval(&mut self, interval: u64) {
        self.interval = interval;
    }
    pub fn write_snapshot(
        &self,
        height: u64,
        accounts: &HashMap<String, Account>,
    ) -> std::io::Result<String> {
        write_snapshot(&self.base, height, accounts)
    }
    pub fn write_diff(
        &self,
        height: u64,
        changes: &HashMap<String, Account>,
        full: &HashMap<String, Account>,
    ) -> std::io::Result<String> {
        write_diff(&self.base, height, changes, full)
    }
    pub fn load_latest(&self) -> std::io::Result<Option<(u64, HashMap<String, Account>, String)>> {
        load_latest(&self.base)
    }
    pub fn list(&self) -> std::io::Result<Vec<u64>> {
        let dir = Path::new(&self.base).join("snapshots");
        let mut hs = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    if let Ok(h) = stem.parse::<u64>() {
                        hs.push(h);
                    }
                }
            }
        }
        hs.sort_unstable();
        Ok(hs)
    }
}

struct SnapshotAccount {
    address: String,
    consumer: u64,
    industrial: u64,
    nonce: u64,
}

struct SnapshotDisk {
    height: u64,
    accounts: Vec<SnapshotAccount>,
    root: String,
}

struct SnapshotDiff {
    height: u64,
    accounts: Vec<SnapshotAccount>,
    root: String,
}

fn encode_snapshot_disk(snap: &SnapshotDisk) -> Vec<u8> {
    encode_snapshot_common(snap.height, &snap.root, &snap.accounts)
}

fn encode_snapshot_diff(diff: &SnapshotDiff) -> Vec<u8> {
    encode_snapshot_common(diff.height, &diff.root, &diff.accounts)
}

fn encode_snapshot_common(height: u64, root: &str, accounts: &[SnapshotAccount]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&(root.len() as u32).to_le_bytes());
    out.extend_from_slice(root.as_bytes());
    out.extend_from_slice(&(accounts.len() as u32).to_le_bytes());
    for account in accounts {
        let address_bytes = account.address.as_bytes();
        out.extend_from_slice(&(address_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(address_bytes);
        out.extend_from_slice(&account.consumer.to_le_bytes());
        out.extend_from_slice(&account.industrial.to_le_bytes());
        out.extend_from_slice(&account.nonce.to_le_bytes());
    }
    out
}

fn decode_snapshot_disk(bytes: &[u8]) -> std::io::Result<SnapshotDisk> {
    decode_snapshot_common(bytes).map(|(height, accounts, root)| SnapshotDisk {
        height,
        accounts,
        root,
    })
}

fn decode_snapshot_diff(bytes: &[u8]) -> std::io::Result<SnapshotDiff> {
    decode_snapshot_common(bytes).map(|(height, accounts, root)| SnapshotDiff {
        height,
        accounts,
        root,
    })
}

fn decode_snapshot_common(bytes: &[u8]) -> std::io::Result<(u64, Vec<SnapshotAccount>, String)> {
    let mut cursor = 0usize;

    fn invalid_data(msg: &'static str) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
    }

    fn read_exact<'a>(
        bytes: &'a [u8],
        cursor: &mut usize,
        len: usize,
    ) -> std::io::Result<&'a [u8]> {
        if bytes.len() < *cursor + len {
            return Err(invalid_data("truncated snapshot data"));
        }
        let slice = &bytes[*cursor..*cursor + len];
        *cursor += len;
        Ok(slice)
    }

    fn read_u32(bytes: &[u8], cursor: &mut usize) -> std::io::Result<u32> {
        let raw = read_exact(bytes, cursor, 4)?;
        Ok(u32::from_le_bytes(raw.try_into().unwrap()))
    }

    fn read_u64(bytes: &[u8], cursor: &mut usize) -> std::io::Result<u64> {
        let raw = read_exact(bytes, cursor, 8)?;
        Ok(u64::from_le_bytes(raw.try_into().unwrap()))
    }

    let height = read_u64(bytes, &mut cursor)?;
    let root_len = read_u32(bytes, &mut cursor)? as usize;
    let root_bytes = read_exact(bytes, &mut cursor, root_len)?;
    let root = String::from_utf8(root_bytes.to_vec())
        .map_err(|_| invalid_data("invalid utf8 in snapshot"))?;

    let account_len = read_u32(bytes, &mut cursor)? as usize;
    let mut accounts = Vec::with_capacity(account_len);
    for _ in 0..account_len {
        let addr_len = read_u32(bytes, &mut cursor)? as usize;
        let addr_bytes = read_exact(bytes, &mut cursor, addr_len)?;
        let address = String::from_utf8(addr_bytes.to_vec())
            .map_err(|_| invalid_data("invalid utf8 in snapshot"))?;
        let consumer = read_u64(bytes, &mut cursor)?;
        let industrial = read_u64(bytes, &mut cursor)?;
        let nonce = read_u64(bytes, &mut cursor)?;
        accounts.push(SnapshotAccount {
            address,
            consumer,
            industrial,
            nonce,
        });
    }

    if cursor != bytes.len() {
        return Err(invalid_data("unexpected trailing snapshot bytes"));
    }

    Ok((height, accounts, root))
}

fn decode_state_snapshot(bytes: &[u8]) -> std::io::Result<([u8; 32], Vec<(Vec<u8>, Vec<u8>)>)> {
    fn invalid_data(msg: &'static str) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
    }

    fn read_exact<'a>(
        bytes: &'a [u8],
        cursor: &mut usize,
        len: usize,
    ) -> std::io::Result<&'a [u8]> {
        if bytes.len() < *cursor + len {
            return Err(invalid_data("truncated snapshot data"));
        }
        let slice = &bytes[*cursor..*cursor + len];
        *cursor += len;
        Ok(slice)
    }

    fn read_u32_be(bytes: &[u8], cursor: &mut usize) -> std::io::Result<u32> {
        let raw = read_exact(bytes, cursor, 4)?;
        Ok(u32::from_be_bytes(raw.try_into().unwrap()))
    }

    let mut cursor = 0usize;
    if bytes.len() < 32 {
        return Err(invalid_data("missing snapshot root"));
    }
    let mut root = [0u8; 32];
    root.copy_from_slice(&bytes[..32]);
    cursor += 32;

    let entry_count = read_u32_be(bytes, &mut cursor)? as usize;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let key_len = read_u32_be(bytes, &mut cursor)? as usize;
        let key = read_exact(bytes, &mut cursor, key_len)?.to_vec();
        let value_len = read_u32_be(bytes, &mut cursor)? as usize;
        let value = read_exact(bytes, &mut cursor, value_len)?.to_vec();
        entries.push((key, value));
    }

    if cursor < bytes.len() {
        let flag = bytes[cursor];
        cursor += 1;
        match flag {
            0 => {}
            1 => {
                let engine_len = read_u32_be(bytes, &mut cursor)? as usize;
                let _ = read_exact(bytes, &mut cursor, engine_len)?;
            }
            _ => return Err(invalid_data("invalid snapshot engine flag")),
        }
    }

    if cursor != bytes.len() {
        return Err(invalid_data("unexpected trailing snapshot bytes"));
    }

    Ok((root, entries))
}

fn merkle_root(accounts: &[SnapshotAccount]) -> String {
    let mut trie = MerkleTrie::new();
    for a in accounts {
        let mut data = Vec::new();
        data.extend_from_slice(&a.consumer.to_le_bytes());
        data.extend_from_slice(&a.industrial.to_le_bytes());
        data.extend_from_slice(&a.nonce.to_le_bytes());
        trie.insert(a.address.as_bytes(), &data);
    }
    hex::encode(trie.root_hash())
}

pub fn state_root(accounts: &HashMap<String, Account>) -> String {
    let mut accs: Vec<SnapshotAccount> = accounts
        .iter()
        .map(|(addr, acc)| SnapshotAccount {
            address: addr.clone(),
            consumer: acc.balance.consumer,
            industrial: acc.balance.industrial,
            nonce: acc.nonce,
        })
        .collect();
    accs.sort_by(|a, b| a.address.cmp(&b.address));
    merkle_root(&accs)
}

pub fn write_snapshot(
    base: &str,
    height: u64,
    accounts: &HashMap<String, Account>,
) -> std::io::Result<String> {
    let start = std::time::Instant::now();
    let res = (|| {
        let mut accs: Vec<SnapshotAccount> = accounts
            .iter()
            .map(|(addr, acc)| SnapshotAccount {
                address: addr.clone(),
                consumer: acc.balance.consumer,
                industrial: acc.balance.industrial,
                nonce: acc.nonce,
            })
            .collect();
        accs.sort_by(|a, b| a.address.cmp(&b.address));
        let root = merkle_root(&accs);
        let snap = SnapshotDisk {
            height,
            accounts: accs,
            root: root.clone(),
        };
        let dir = Path::new(base).join("snapshots");
        fs::create_dir_all(&dir)?;
        let file = dir.join(format!("{:010}.bin", height));
        let bytes = encode_snapshot_disk(&snap);
        fs::write(&file, bytes)?;
        // Rotate old snapshots and diffs
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    if let Ok(h) = stem.parse::<u64>() {
                        if h < height {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
        Ok(root)
    })();
    #[cfg(feature = "telemetry")]
    {
        SNAPSHOT_DURATION_SECONDS.observe(start.elapsed().as_secs_f64());
        if res.is_err() {
            SNAPSHOT_FAIL_TOTAL.inc();
        }
    }
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = start;
    }
    res
}

pub fn write_diff(
    base: &str,
    height: u64,
    changes: &HashMap<String, Account>,
    full_accounts: &HashMap<String, Account>,
) -> std::io::Result<String> {
    let start = std::time::Instant::now();
    let res = (|| {
        let mut accs: Vec<SnapshotAccount> = changes
            .iter()
            .map(|(addr, acc)| SnapshotAccount {
                address: addr.clone(),
                consumer: acc.balance.consumer,
                industrial: acc.balance.industrial,
                nonce: acc.nonce,
            })
            .collect();
        accs.sort_by(|a, b| a.address.cmp(&b.address));
        let root = state_root(full_accounts);
        let diff = SnapshotDiff {
            height,
            accounts: accs,
            root: root.clone(),
        };
        let dir = Path::new(base).join("snapshots");
        fs::create_dir_all(&dir)?;
        let file = dir.join(format!("{:010}.diff", height));
        let bytes = encode_snapshot_diff(&diff);
        fs::write(file, bytes)?;
        Ok(root)
    })();
    #[cfg(feature = "telemetry")]
    {
        SNAPSHOT_DURATION_SECONDS.observe(start.elapsed().as_secs_f64());
        if res.is_err() {
            SNAPSHOT_FAIL_TOTAL.inc();
        }
    }
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = start;
    }
    res
}

pub fn load_latest(base: &str) -> std::io::Result<Option<(u64, HashMap<String, Account>, String)>> {
    let start = std::time::Instant::now();
    let res = (|| {
        let dir = Path::new(base).join("snapshots");
        let mut latest: Option<(u64, HashMap<String, Account>, String)> = None;
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if let Ok(height) = stem.parse::<u64>() {
                    let bytes = match fs::read(&path) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    if let Ok(snap) = decode_snapshot_disk(&bytes) {
                        if latest.as_ref().map_or(true, |(h, _, _)| height > *h) {
                            let accounts_map: HashMap<String, Account> = snap
                                .accounts
                                .into_iter()
                                .map(|a| {
                                    (
                                        a.address.clone(),
                                        Account {
                                            address: a.address,
                                            balance: TokenBalance {
                                                consumer: a.consumer,
                                                industrial: a.industrial,
                                            },
                                            nonce: a.nonce,
                                            pending_consumer: 0,
                                            pending_industrial: 0,
                                            pending_nonce: 0,
                                            pending_nonces: HashSet::new(),
                                            sessions: Vec::new(),
                                        },
                                    )
                                })
                                .collect();
                            latest = Some((height, accounts_map, snap.root));
                        }
                    }
                }
            }
        }
        if let Some((h, mut accounts, mut root)) = latest {
            // Apply any diffs newer than the snapshot
            let mut diffs: Vec<(u64, SnapshotDiff)> = Vec::new();
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("diff") {
                        continue;
                    }
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(height) = stem.parse::<u64>() {
                            if height > h {
                                if let Ok(bytes) = fs::read(&path) {
                                    if let Ok(diff) = decode_snapshot_diff(&bytes) {
                                        diffs.push((height, diff));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            diffs.sort_by_key(|(h, _)| *h);
            let mut current_height = h;
            for (height, diff) in diffs {
                for a in diff.accounts {
                    accounts.insert(
                        a.address.clone(),
                        Account {
                            address: a.address,
                            balance: TokenBalance {
                                consumer: a.consumer,
                                industrial: a.industrial,
                            },
                            nonce: a.nonce,
                            pending_consumer: 0,
                            pending_industrial: 0,
                            pending_nonce: 0,
                            pending_nonces: HashSet::new(),
                            sessions: Vec::new(),
                        },
                    );
                }
                root = diff.root;
                current_height = height;
            }
            return Ok(Some((current_height, accounts, root)));
        }
        Ok(None)
    })();
    #[cfg(feature = "telemetry")]
    {
        SNAPSHOT_DURATION_SECONDS.observe(start.elapsed().as_secs_f64());
        if res.is_err() {
            SNAPSHOT_FAIL_TOTAL.inc();
        }
    }
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = start;
    }
    res
}

pub fn load_file(path: &str) -> std::io::Result<(u64, HashMap<String, Account>, String)> {
    let data = fs::read(path)?;
    let (root, entries) = decode_state_snapshot(&data)?;
    let mut accounts = HashMap::new();
    for (key, value) in entries {
        let address = String::from_utf8(key)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid address"))?;
        if value.len() < 24 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "snapshot entry too short",
            ));
        }
        let consumer = u64::from_le_bytes(value[0..8].try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "consumer decode")
        })?);
        let industrial = u64::from_le_bytes(value[8..16].try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "industrial decode")
        })?);
        let nonce =
            u64::from_le_bytes(value[16..24].try_into().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "nonce decode")
            })?);
        accounts.insert(
            address.clone(),
            Account {
                address,
                balance: crate::TokenBalance {
                    consumer,
                    industrial,
                },
                nonce,
                pending_consumer: 0,
                pending_industrial: 0,
                pending_nonce: 0,
                pending_nonces: HashSet::new(),
                sessions: Vec::new(),
            },
        );
    }
    let height = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "snapshot filename missing height",
            )
        })?
        .parse::<u64>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok((height, accounts, hex::encode(root)))
}

pub fn account_proof(
    accounts: &HashMap<String, Account>,
    address: &str,
) -> Option<Vec<(String, bool)>> {
    let mut trie = MerkleTrie::new();
    for (addr, acc) in accounts {
        let mut data = Vec::new();
        data.extend_from_slice(&acc.balance.consumer.to_le_bytes());
        data.extend_from_slice(&acc.balance.industrial.to_le_bytes());
        data.extend_from_slice(&acc.nonce.to_le_bytes());
        trie.insert(addr.as_bytes(), &data);
    }
    let proof = trie.prove(address.as_bytes())?;
    Some(
        proof
            .0
            .into_iter()
            .map(|(h, is_left)| (hex::encode(h), is_left))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile::tempdir;

    #[test]
    fn load_file_rebuilds_accounts_from_entries() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("0000000042.bin");

        let mut value = Vec::new();
        value.extend_from_slice(&11u64.to_le_bytes());
        value.extend_from_slice(&7u64.to_le_bytes());
        value.extend_from_slice(&3u64.to_le_bytes());

        let mut trie = MerkleTrie::new();
        trie.insert(b"alice", &value);
        let root = trie.root_hash();

        let snapshot = Snapshot {
            root,
            entries: vec![(b"alice".to_vec(), value)],
            engine_backend: None,
        };
        let bytes = bincode::serialize(&snapshot).expect("serialize snapshot");
        fs::write(&path, bytes).expect("write snapshot");

        let (height, accounts, root_hex) = load_file(path.to_str().unwrap()).expect("load");
        assert_eq!(height, 42);
        assert_eq!(root_hex, hex::encode(root));
        let alice = accounts.get("alice").expect("alice present");
        assert_eq!(alice.address, "alice");
        assert_eq!(alice.balance.consumer, 11);
        assert_eq!(alice.balance.industrial, 7);
        assert_eq!(alice.nonce, 3);
        assert_eq!(alice.pending_consumer, 0);
        assert_eq!(alice.pending_industrial, 0);
        assert_eq!(alice.pending_nonce, 0);
    }
}

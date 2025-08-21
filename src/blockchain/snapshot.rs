use crate::{Account, TokenBalance};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct SnapshotAccount {
    address: String,
    consumer: u64,
    industrial: u64,
    nonce: u64,
}

#[derive(Serialize, Deserialize)]
struct SnapshotDisk {
    height: u64,
    accounts: Vec<SnapshotAccount>,
    root: String,
}

#[derive(Serialize, Deserialize)]
struct SnapshotDiff {
    height: u64,
    accounts: Vec<SnapshotAccount>,
    root: String,
}

fn merkle_root(accounts: &[SnapshotAccount]) -> String {
    let mut leaves: Vec<[u8; 32]> = accounts
        .iter()
        .map(|a| {
            let mut h = Hasher::new();
            h.update(a.address.as_bytes());
            h.update(&a.consumer.to_le_bytes());
            h.update(&a.industrial.to_le_bytes());
            h.update(&a.nonce.to_le_bytes());
            *h.finalize().as_bytes()
        })
        .collect();
    if leaves.is_empty() {
        return String::new();
    }
    while leaves.len() > 1 {
        if leaves.len() % 2 == 1 {
            let last = leaves.last().copied().unwrap_or([0u8; 32]);
            leaves.push(last);
        }
        let mut next = Vec::with_capacity(leaves.len() / 2);
        for pair in leaves.chunks(2) {
            let mut h = Hasher::new();
            h.update(&pair[0]);
            h.update(&pair[1]);
            next.push(*h.finalize().as_bytes());
        }
        leaves = next;
    }
    blake3::Hash::from(leaves[0]).to_hex().to_string()
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
    let bytes = bincode::serialize(&snap).unwrap_or_else(|e| panic!("snapshot serialize: {e}"));
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
}

pub fn write_diff(
    base: &str,
    height: u64,
    changes: &HashMap<String, Account>,
    full_accounts: &HashMap<String, Account>,
) -> std::io::Result<String> {
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
    let bytes = bincode::serialize(&diff).unwrap_or_else(|e| panic!("diff serialize: {e}"));
    fs::write(file, bytes)?;
    Ok(root)
}

pub fn load_latest(base: &str) -> std::io::Result<Option<(u64, HashMap<String, Account>, String)>> {
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
                if let Ok(snap) = bincode::deserialize::<SnapshotDisk>(&bytes) {
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
                                if let Ok(diff) = bincode::deserialize::<SnapshotDiff>(&bytes) {
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
                    },
                );
            }
            root = diff.root;
            current_height = height;
        }
        return Ok(Some((current_height, accounts, root)));
    }
    Ok(None)
}

pub fn account_proof(
    accounts: &HashMap<String, Account>,
    address: &str,
) -> Option<Vec<(String, bool)>> {
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
    let mut leaves: Vec<[u8; 32]> = accs
        .iter()
        .map(|a| {
            let mut h = Hasher::new();
            h.update(a.address.as_bytes());
            h.update(&a.consumer.to_le_bytes());
            h.update(&a.industrial.to_le_bytes());
            h.update(&a.nonce.to_le_bytes());
            *h.finalize().as_bytes()
        })
        .collect();
    let mut index = accs.iter().position(|a| a.address == address)?;
    if leaves.is_empty() {
        return None;
    }
    let mut proof = Vec::new();
    while leaves.len() > 1 {
        if leaves.len() % 2 == 1 {
            let last = leaves.last().copied().unwrap_or([0u8; 32]);
            leaves.push(last);
        }
        let sibling_index = if index % 2 == 0 { index + 1 } else { index - 1 };
        let sibling = leaves.get(sibling_index).copied().unwrap_or([0u8; 32]);
        let is_left = index % 2 == 1;
        proof.push((blake3::Hash::from(sibling).to_hex().to_string(), is_left));
        let mut next = Vec::with_capacity(leaves.len() / 2);
        for pair in leaves.chunks(2) {
            let mut h = Hasher::new();
            h.update(&pair[0]);
            h.update(&pair[1]);
            next.push(*h.finalize().as_bytes());
        }
        leaves = next;
        index /= 2;
    }
    Some(proof)
}

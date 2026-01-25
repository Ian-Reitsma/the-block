use std::collections::VecDeque;

use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};
use ledger::address::ShardId;

use crate::{simple_db::SimpleDb as Db, transaction::FeeLane, util::binary_codec};

const L2_CADENCE_MILLIS: u64 = 4_000;
const L3_CADENCE_MILLIS: u64 = 16_000;
const ROOT_QUEUE_DEPTH: usize = 1024;

/// Size class for micro-shard roots.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum RootSizeClass {
    L2,
    L3,
}

impl RootSizeClass {
    pub fn cadence_millis(self) -> u64 {
        match self {
            RootSizeClass::L2 => L2_CADENCE_MILLIS,
            RootSizeClass::L3 => L3_CADENCE_MILLIS,
        }
    }

    pub fn as_byte(self) -> u8 {
        match self {
            RootSizeClass::L2 => 1,
            RootSizeClass::L3 => 2,
        }
    }
}

/// Canonical entry for a single micro-shard root.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MicroShardRootEntry {
    pub root_hash: [u8; 32],
    pub shard_id: ShardId,
    pub lane: FeeLane,
    pub available_until: u64,
    pub payload_bytes: u32,
}

/// Deterministic bundle of micro-shard roots emitted for a slot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RootBundle {
    pub slot: u64,
    pub size_class: RootSizeClass,
    pub bundle_hash: [u8; 32],
    pub entries: Vec<MicroShardRootEntry>,
}

impl RootBundle {
    pub fn new(slot: u64, size_class: RootSizeClass, entries: Vec<MicroShardRootEntry>) -> Self {
        let bundle_hash = Self::compute_hash(slot, size_class, &entries);
        Self {
            slot,
            size_class,
            bundle_hash,
            entries,
        }
    }

    pub fn compute_hash(
        slot: u64,
        size_class: RootSizeClass,
        entries: &[MicroShardRootEntry],
    ) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&slot.to_le_bytes());
        h.update(&[size_class.as_byte()]);
        h.update(&(entries.len() as u32).to_le_bytes());
        for entry in entries {
            h.update(&entry.root_hash);
            h.update(&(entry.shard_id as u32).to_le_bytes());
            h.update(&[entry.lane as u8]);
            h.update(&entry.available_until.to_le_bytes());
            h.update(&entry.payload_bytes.to_le_bytes());
        }
        h.finalize().into()
    }
}

/// Lightweight summary persisted to the manifest for macro-block checkpoints.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RootBundleSummary {
    pub slot: u64,
    pub size_class: RootSizeClass,
    pub bundle_hash: [u8; 32],
    pub entry_count: u32,
    pub available_until: u64,
}

impl From<&RootBundle> for RootBundleSummary {
    fn from(bundle: &RootBundle) -> Self {
        let available_until = bundle
            .entries
            .iter()
            .map(|e| e.available_until)
            .max()
            .unwrap_or(0);
        Self {
            slot: bundle.slot,
            size_class: bundle.size_class,
            bundle_hash: bundle.bundle_hash,
            entry_count: bundle.entries.len() as u32,
            available_until,
        }
    }
}

/// Append-only manifest for anchored root bundles.
#[derive(Default)]
pub struct RootManifest {
    recent: Vec<RootBundleSummary>,
}

impl RootManifest {
    pub fn record(&mut self, db: &mut Db, bundle: &RootBundle) {
        let summary = RootBundleSummary::from(bundle);
        let key = format!(
            "root_manifest:{}:{}",
            summary.size_class.as_byte(),
            summary.slot
        );
        if let Ok(encoded) = binary_codec::serialize(bundle) {
            let _ = db.insert(&key, encoded);
        }
        self.recent.push(summary);
        if self.recent.len() > ROOT_QUEUE_DEPTH {
            self.recent.remove(0);
        }
    }

    pub fn drain_recent(&mut self) -> Vec<RootBundleSummary> {
        std::mem::take(&mut self.recent)
    }
}

/// Deterministic assembler replacing the legacy blob scheduler.
#[derive(Default)]
pub struct RootAssembler {
    l2_queue: VecDeque<MicroShardRootEntry>,
    l3_queue: VecDeque<MicroShardRootEntry>,
    last_slot_l2: u64,
    last_slot_l3: u64,
    max_queue: usize,
}

impl RootAssembler {
    pub fn new() -> Self {
        Self {
            l2_queue: VecDeque::new(),
            l3_queue: VecDeque::new(),
            last_slot_l2: 0,
            last_slot_l3: 0,
            max_queue: ROOT_QUEUE_DEPTH,
        }
    }

    pub fn enqueue(&mut self, entry: MicroShardRootEntry, size_class: RootSizeClass) {
        let queue = match size_class {
            RootSizeClass::L2 => &mut self.l2_queue,
            RootSizeClass::L3 => &mut self.l3_queue,
        };
        queue.push_back(entry);
        if queue.len() > self.max_queue {
            queue.pop_front();
        }
    }

    fn next_slot(&self, timestamp_millis: u64, class: RootSizeClass) -> u64 {
        timestamp_millis / class.cadence_millis()
    }

    pub fn ready_bundles(&mut self, timestamp_millis: u64) -> Vec<RootBundle> {
        let mut bundles = Vec::new();
        let slot_l2 = self.next_slot(timestamp_millis, RootSizeClass::L2);
        if slot_l2 > self.last_slot_l2 && !self.l2_queue.is_empty() {
            let entries: Vec<MicroShardRootEntry> = self.l2_queue.drain(..).collect();
            bundles.push(RootBundle::new(slot_l2, RootSizeClass::L2, entries));
            self.last_slot_l2 = slot_l2;
        }
        let slot_l3 = self.next_slot(timestamp_millis, RootSizeClass::L3);
        if slot_l3 > self.last_slot_l3 && !self.l3_queue.is_empty() {
            let entries: Vec<MicroShardRootEntry> = self.l3_queue.drain(..).collect();
            bundles.push(RootBundle::new(slot_l3, RootSizeClass::L3, entries));
            self.last_slot_l3 = slot_l3;
        }
        bundles
    }
}

impl Default for RootSizeClass {
    fn default() -> Self {
        RootSizeClass::L2
    }
}

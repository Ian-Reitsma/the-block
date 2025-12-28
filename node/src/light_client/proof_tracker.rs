use std::convert::TryInto;
use std::path::Path;

use crate::util::binary_struct::{self, DecodeError as BinaryDecodeError};
use crate::{simple_db::names, Block, SimpleDb, TokenAmount};
use foundation_serialization::binary_cursor::{Reader as BinaryReader, Writer as BinaryWriter};

const RELAYER_PREFIX: &str = "relayers/";
const RECEIPT_PREFIX: &str = "receipts/";
const META_PENDING_TOTAL: &str = "meta/pending_total";

#[derive(Debug, Clone, Default)]
struct StoredRelayer {
    pending: u64,
    total_proofs: u64,
    total_claimed: u64,
    last_claim_height: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct ClaimReceipt {
    amount: u64,
    relayers: Vec<RelayerClaim>,
}

#[derive(Debug, Clone)]
struct RelayerClaim {
    id: Vec<u8>,
    amount: u64,
    prev_last_claim_height: Option<u64>,
}

/// Snapshot of a relayer's rebate accounting suitable for CLI inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayerInfo {
    pub pending: u64,
    pub total_proofs: u64,
    pub total_claimed: u64,
    pub last_claim_height: Option<u64>,
}

/// Aggregate rebate state exported for monitoring and CLI inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebateSnapshot {
    pub pending_total: u64,
    pub relayers: Vec<(Vec<u8>, RelayerInfo)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptRelayer {
    pub id: Vec<u8>,
    pub amount: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptEntry {
    pub height: u64,
    pub amount: u64,
    pub relayers: Vec<ReceiptRelayer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptPage {
    pub receipts: Vec<ReceiptEntry>,
    pub next: Option<u64>,
}

pub struct ProofTracker {
    db: SimpleDb,
}

impl ProofTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Self {
        let path_ref = path.as_ref();
        let db_path = path_ref.to_string_lossy().into_owned();
        Self::with_db(SimpleDb::open_named(names::LIGHT_CLIENT_PROOFS, &db_path))
    }

    pub fn with_db(db: SimpleDb) -> Self {
        let tracker = Self { db };
        tracker.update_pending_metric();
        tracker
    }

    fn pending_total(&self) -> u64 {
        self.db
            .get(META_PENDING_TOTAL)
            .and_then(|v| v.as_slice().try_into().ok().map(u64::from_le_bytes))
            .unwrap_or(0)
    }

    fn set_pending_total(&mut self, value: u64) {
        let _ = self
            .db
            .insert(META_PENDING_TOTAL, value.to_le_bytes().to_vec());
        self.update_pending_metric();
    }

    fn load_relayer(&self, key: &str) -> StoredRelayer {
        self.db
            .get(key)
            .and_then(|bytes| decode_stored_relayer(&bytes).ok())
            .unwrap_or_default()
    }

    fn store_relayer(&mut self, key: &str, value: &StoredRelayer) {
        let bytes = encode_stored_relayer(value);
        let _ = self.db.insert(key, bytes);
    }

    fn relayer_key(id: &[u8]) -> String {
        format!("{}{}", RELAYER_PREFIX, crypto_suite::hex::encode(id))
    }

    fn receipt_key(height: u64) -> String {
        format!("{}{:016x}", RECEIPT_PREFIX, height)
    }

    fn load_receipt_by_key(&self, key: &str) -> Option<ClaimReceipt> {
        self.db
            .get(key)
            .and_then(|bytes| match decode_claim_receipt(&bytes) {
                Ok(receipt) => Some(receipt),
                Err(_) if bytes.len() == 8 => {
                    let arr: [u8; 8] = bytes.as_slice().try_into().ok()?;
                    Some(ClaimReceipt {
                        amount: u64::from_le_bytes(arr),
                        relayers: Vec::new(),
                    })
                }
                Err(_) => None,
            })
    }

    fn load_receipt(&self, height: u64) -> Option<ClaimReceipt> {
        let key = Self::receipt_key(height);
        self.load_receipt_by_key(&key)
    }

    fn store_receipt(&mut self, height: u64, receipt: &ClaimReceipt) {
        let bytes = encode_claim_receipt(receipt);
        let key = Self::receipt_key(height);
        let _ = self.db.insert(&key, bytes);
    }

    fn remove_receipt(&mut self, height: u64) {
        let key = Self::receipt_key(height);
        let _ = self.db.remove(&key);
    }

    fn update_pending_metric(&self) {
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::PROOF_REBATES_PENDING_TOTAL.set(self.pending_total() as i64);
        }
    }

    /// Record `proofs` delivered by `id`, crediting `amount` BLOCK micro-rebates.
    /// Returns the amount actually recorded (0 if suppressed).
    pub fn record(&mut self, id: &[u8], proofs: u64, amount: u64) -> u64 {
        if proofs == 0 || amount == 0 {
            return 0;
        }
        let key = Self::relayer_key(id);
        let mut entry = self.load_relayer(&key);
        entry.pending = entry.pending.saturating_add(amount);
        entry.total_proofs = entry.total_proofs.saturating_add(proofs);
        self.store_relayer(&key, &entry);
        let total = self.pending_total().saturating_add(amount);
        self.set_pending_total(total);
        amount
    }

    /// Claim all pending rebates and mark them consumed at `height`.
    pub fn claim_all(&mut self, height: u64) -> u64 {
        if self.load_receipt(height).is_some() {
            return 0;
        }
        let total = self.pending_total();
        if total == 0 {
            self.store_receipt(height, &ClaimReceipt::default());
            return 0;
        }
        let mut receipt = ClaimReceipt {
            amount: total,
            relayers: Vec::new(),
        };
        let mut relayer_keys = self.db.keys_with_prefix(RELAYER_PREFIX);
        relayer_keys.sort();
        for key in relayer_keys {
            let mut entry = self.load_relayer(&key);
            if entry.pending == 0 {
                continue;
            }
            let id_hex = key.trim_start_matches(RELAYER_PREFIX);
            if let Ok(id) = crypto_suite::hex::decode(id_hex) {
                receipt.relayers.push(RelayerClaim {
                    id,
                    amount: entry.pending,
                    prev_last_claim_height: entry.last_claim_height,
                });
            }
            entry.total_claimed = entry.total_claimed.saturating_add(entry.pending);
            entry.last_claim_height = Some(height);
            entry.pending = 0;
            self.store_relayer(&key, &entry);
        }
        self.set_pending_total(0);
        self.store_receipt(height, &receipt);
        if total > 0 {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::PROOF_REBATES_CLAIMED_TOTAL.inc();
                crate::telemetry::PROOF_REBATES_AMOUNT_TOTAL.inc_by(total);
            }
        }
        total
    }

    /// Undo a previously recorded claim, restoring pending balances.
    pub fn rollback_claim(&mut self, height: u64) -> u64 {
        let receipt_key = Self::receipt_key(height);
        let Some(receipt) = self.load_receipt_by_key(&receipt_key) else {
            return 0;
        };
        if receipt.amount == 0 && receipt.relayers.is_empty() {
            self.remove_receipt(height);
            return 0;
        }
        for claim in &receipt.relayers {
            let key = Self::relayer_key(&claim.id);
            let mut entry = self.load_relayer(&key);
            entry.pending = entry.pending.saturating_add(claim.amount);
            entry.total_claimed = entry.total_claimed.saturating_sub(claim.amount);
            entry.last_claim_height = claim.prev_last_claim_height;
            self.store_relayer(&key, &entry);
        }
        let total = receipt.amount;
        let new_total = self.pending_total().saturating_add(total);
        self.set_pending_total(new_total);
        self.remove_receipt(height);
        total
    }

    /// Return a snapshot of all tracked relayers and pending totals.
    pub fn snapshot(&self) -> RebateSnapshot {
        let keys = self.db.keys_with_prefix(RELAYER_PREFIX);
        let mut relayers = Vec::with_capacity(keys.len());
        for key in keys {
            let entry = self.load_relayer(&key);
            let hex_id = key.trim_start_matches(RELAYER_PREFIX);
            if let Ok(bytes) = crypto_suite::hex::decode(hex_id) {
                relayers.push((
                    bytes,
                    RelayerInfo {
                        pending: entry.pending,
                        total_proofs: entry.total_proofs,
                        total_claimed: entry.total_claimed,
                        last_claim_height: entry.last_claim_height,
                    },
                ));
            }
        }
        relayers.sort_by(|a, b| a.0.cmp(&b.0));
        RebateSnapshot {
            pending_total: self.pending_total(),
            relayers,
        }
    }

    /// Return a paginated view of stored claim receipts optionally filtered by relayer ID.
    pub fn receipt_history(
        &self,
        relayer: Option<&[u8]>,
        cursor: Option<u64>,
        limit: usize,
    ) -> ReceiptPage {
        if limit == 0 {
            return ReceiptPage {
                receipts: Vec::new(),
                next: None,
            };
        }

        let mut keys = self.db.keys_with_prefix(RECEIPT_PREFIX);
        keys.sort();
        keys.reverse();

        let mut receipts = Vec::new();
        for key in keys {
            let Some(height_hex) = key.strip_prefix(RECEIPT_PREFIX) else {
                continue;
            };
            let Ok(height) = u64::from_str_radix(height_hex, 16) else {
                continue;
            };
            if let Some(cursor_height) = cursor {
                if height >= cursor_height {
                    continue;
                }
            }
            let Some(receipt) = self.load_receipt_by_key(&key) else {
                continue;
            };
            if receipt.amount == 0 && receipt.relayers.is_empty() {
                continue;
            }
            let filtered: Vec<ReceiptRelayer> = receipt
                .relayers
                .iter()
                .filter_map(|claim| {
                    if relayer
                        .map(|needle| claim.id.as_slice() == needle)
                        .unwrap_or(true)
                    {
                        Some(ReceiptRelayer {
                            id: claim.id.clone(),
                            amount: claim.amount,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            if relayer.is_some() && filtered.is_empty() {
                continue;
            }
            let amount = if relayer.is_some() {
                filtered.iter().map(|r| r.amount).sum()
            } else {
                receipt.amount
            };
            receipts.push(ReceiptEntry {
                height,
                amount,
                relayers: filtered,
            });
            if receipts.len() >= limit {
                break;
            }
        }
        let next = receipts.last().map(|entry| entry.height);
        ReceiptPage { receipts, next }
    }
}

impl Default for ProofTracker {
    fn default() -> Self {
        Self::with_db(SimpleDb::default())
    }
}

/// Apply `amount` rebates to block coinbase.
pub fn apply_rebates(block: &mut Block, amount: u64) {
    if amount > 0 {
        block.coinbase_block = block
            .coinbase_block
            .saturating_add(TokenAmount::new(amount));
        block.proof_rebate = block.proof_rebate.saturating_add(TokenAmount::new(amount));
    }
}

fn encode_stored_relayer(value: &StoredRelayer) -> Vec<u8> {
    let mut writer = BinaryWriter::with_capacity(64);
    writer.write_struct(|st| {
        st.field_u64("pending", value.pending);
        st.field_u64("total_proofs", value.total_proofs);
        st.field_u64("total_claimed", value.total_claimed);
        st.field_option_u64("last_claim_height", value.last_claim_height);
    });
    writer.finish()
}

fn decode_stored_relayer(bytes: &[u8]) -> Result<StoredRelayer, BinaryDecodeError> {
    let mut reader = BinaryReader::new(bytes);
    let mut pending = None;
    let mut total_proofs = None;
    let mut total_claimed = None;
    let mut last_claim_height = None;

    binary_struct::decode_struct(&mut reader, Some(4), |key, reader| match key {
        "pending" => binary_struct::assign_once(
            &mut pending,
            reader.read_u64().map_err(BinaryDecodeError::from)?,
            "pending",
        ),
        "total_proofs" => binary_struct::assign_once(
            &mut total_proofs,
            reader.read_u64().map_err(BinaryDecodeError::from)?,
            "total_proofs",
        ),
        "total_claimed" => binary_struct::assign_once(
            &mut total_claimed,
            reader.read_u64().map_err(BinaryDecodeError::from)?,
            "total_claimed",
        ),
        "last_claim_height" => binary_struct::assign_once(
            &mut last_claim_height,
            reader.read_option_with(|reader| reader.read_u64().map_err(BinaryDecodeError::from))?,
            "last_claim_height",
        ),
        other => Err(BinaryDecodeError::UnknownField(other.into())),
    })?;

    binary_struct::ensure_exhausted(&reader)?;

    Ok(StoredRelayer {
        pending: pending.ok_or(BinaryDecodeError::MissingField("pending"))?,
        total_proofs: total_proofs.ok_or(BinaryDecodeError::MissingField("total_proofs"))?,
        total_claimed: total_claimed.ok_or(BinaryDecodeError::MissingField("total_claimed"))?,
        last_claim_height: last_claim_height.unwrap_or(None),
    })
}

fn encode_claim_receipt(value: &ClaimReceipt) -> Vec<u8> {
    let mut writer = BinaryWriter::with_capacity(128);
    writer.write_struct(|st| {
        st.field_u64("amount", value.amount);
        st.field_vec_with("relayers", &value.relayers, |writer, claim| {
            writer.write_struct(|inner| {
                inner.field_bytes("id", &claim.id);
                inner.field_u64("amount", claim.amount);
                inner.field_option_u64("prev_last_claim_height", claim.prev_last_claim_height);
            });
        });
    });
    writer.finish()
}

fn decode_relayer_claim(reader: &mut BinaryReader<'_>) -> Result<RelayerClaim, BinaryDecodeError> {
    let mut id = None;
    let mut amount = None;
    let mut prev_last_claim_height: Option<Option<u64>> = None;

    binary_struct::decode_struct(reader, Some(3), |key, reader| match key {
        "id" => binary_struct::assign_once(
            &mut id,
            reader.read_bytes().map_err(BinaryDecodeError::from)?,
            "id",
        ),
        "amount" => binary_struct::assign_once(
            &mut amount,
            reader.read_u64().map_err(BinaryDecodeError::from)?,
            "amount",
        ),
        "prev_last_claim_height" => binary_struct::assign_once(
            &mut prev_last_claim_height,
            reader.read_option_with(|reader| reader.read_u64().map_err(BinaryDecodeError::from))?,
            "prev_last_claim_height",
        ),
        other => Err(BinaryDecodeError::UnknownField(other.into())),
    })?;

    Ok(RelayerClaim {
        id: id.ok_or(BinaryDecodeError::MissingField("id"))?,
        amount: amount.ok_or(BinaryDecodeError::MissingField("amount"))?,
        prev_last_claim_height: prev_last_claim_height.unwrap_or(None),
    })
}

fn decode_claim_receipt(bytes: &[u8]) -> Result<ClaimReceipt, BinaryDecodeError> {
    let mut reader = BinaryReader::new(bytes);
    let mut amount = None;
    let mut relayers = None;

    binary_struct::decode_struct(&mut reader, Some(2), |key, reader| match key {
        "amount" => binary_struct::assign_once(
            &mut amount,
            reader.read_u64().map_err(BinaryDecodeError::from)?,
            "amount",
        ),
        "relayers" => binary_struct::assign_once(
            &mut relayers,
            reader.read_vec_with(|reader| decode_relayer_claim(reader))?,
            "relayers",
        ),
        other => Err(BinaryDecodeError::UnknownField(other.into())),
    })?;

    binary_struct::ensure_exhausted(&reader)?;

    Ok(ClaimReceipt {
        amount: amount.ok_or(BinaryDecodeError::MissingField("amount"))?,
        relayers: relayers.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::binary_cursor::Reader as BinaryReader;
    use sys::tempfile::tempdir;

    fn tracker_with_tempdir() -> ProofTracker {
        let dir = tempdir().expect("tempdir");
        let base = dir.into_path();
        let path = base.join("rebates");
        ProofTracker::open(path)
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("rebates");
        {
            let mut tracker = ProofTracker::open(&path);
            tracker.record(&[1, 2, 3], 1, 5);
            assert_eq!(tracker.pending_total(), 5);
        }
        let reopened = ProofTracker::open(&path);
        let snap = reopened.snapshot();
        assert_eq!(snap.pending_total, 5);
        assert_eq!(snap.relayers.len(), 1);
        assert_eq!(snap.relayers[0].1.pending, 5);
    }

    #[test]
    fn stored_relayer_encoding_matches_legacy() {
        let relayer = StoredRelayer {
            pending: 10,
            total_proofs: 4,
            total_claimed: 7,
            last_claim_height: Some(42),
        };

        let manual = encode_stored_relayer(&relayer);
        let mut reader = BinaryReader::new(&manual);
        assert_eq!(reader.read_u64().expect("field count"), 4);
        assert_eq!(reader.read_string().expect("pending key"), "pending");
        assert_eq!(reader.read_u64().expect("pending"), relayer.pending);
        assert_eq!(
            reader.read_string().expect("total_proofs key"),
            "total_proofs"
        );
        assert_eq!(
            reader.read_u64().expect("total_proofs"),
            relayer.total_proofs
        );
        assert_eq!(
            reader.read_string().expect("total_claimed key"),
            "total_claimed"
        );
        assert_eq!(
            reader.read_u64().expect("total_claimed"),
            relayer.total_claimed
        );
        assert_eq!(
            reader.read_string().expect("last_claim_height key"),
            "last_claim_height"
        );
        assert!(reader.read_bool().expect("last_claim_height present"));
        assert_eq!(
            reader.read_u64().expect("last_claim_height"),
            relayer.last_claim_height.unwrap()
        );

        let decoded = decode_stored_relayer(&manual).expect("decode relayer");
        assert_eq!(decoded.pending, relayer.pending);
        assert_eq!(decoded.total_proofs, relayer.total_proofs);
        assert_eq!(decoded.total_claimed, relayer.total_claimed);
        assert_eq!(decoded.last_claim_height, relayer.last_claim_height);
    }

    #[test]
    fn claim_receipt_encoding_matches_legacy() {
        let claim = ClaimReceipt {
            amount: 55,
            relayers: vec![
                RelayerClaim {
                    id: vec![1, 2, 3],
                    amount: 15,
                    prev_last_claim_height: Some(9),
                },
                RelayerClaim {
                    id: vec![4, 5],
                    amount: 40,
                    prev_last_claim_height: None,
                },
            ],
        };

        let manual = encode_claim_receipt(&claim);
        let mut reader = BinaryReader::new(&manual);
        assert_eq!(reader.read_u64().expect("field count"), 2);
        assert_eq!(reader.read_string().expect("amount key"), "amount");
        assert_eq!(reader.read_u64().expect("amount"), claim.amount);
        assert_eq!(reader.read_string().expect("relayers key"), "relayers");
        let relayer_count = reader.read_u64().expect("relayer len");
        assert_eq!(relayer_count, claim.relayers.len() as u64);
        for expected in &claim.relayers {
            assert_eq!(reader.read_u64().expect("relayer field count"), 3);
            assert_eq!(reader.read_string().expect("id key"), "id");
            let id_len = reader.read_u64().expect("id len");
            assert_eq!(id_len, expected.id.len() as u64);
            let id_bytes = reader.read_exact(id_len as usize).expect("id bytes");
            assert_eq!(id_bytes, expected.id.as_slice());
            assert_eq!(reader.read_string().expect("amount key"), "amount");
            assert_eq!(reader.read_u64().expect("relayer amount"), expected.amount);
            assert_eq!(
                reader.read_string().expect("prev key"),
                "prev_last_claim_height"
            );
            let has_prev = reader.read_bool().expect("prev present");
            if let Some(height) = expected.prev_last_claim_height {
                assert!(has_prev);
                assert_eq!(reader.read_u64().expect("prev value"), height);
            } else {
                assert!(!has_prev);
            }
        }

        let decoded = decode_claim_receipt(&manual).expect("decode receipt");
        assert_eq!(decoded.amount, claim.amount);
        assert_eq!(decoded.relayers.len(), claim.relayers.len());
        for (lhs, rhs) in decoded.relayers.iter().zip(&claim.relayers) {
            assert_eq!(lhs.id, rhs.id);
            assert_eq!(lhs.amount, rhs.amount);
            assert_eq!(lhs.prev_last_claim_height, rhs.prev_last_claim_height);
        }
    }

    #[test]
    fn duplicate_claim_prevented() {
        let mut tracker = tracker_with_tempdir();
        tracker.record(&[9], 2, 10);
        let first = tracker.claim_all(42);
        assert_eq!(first, 10);
        let second = tracker.claim_all(42);
        assert_eq!(second, 0);
    }

    #[test]
    fn rollback_restores_pending() {
        let mut tracker = tracker_with_tempdir();
        let relayer = vec![7, 8, 9];
        tracker.record(&relayer, 4, 12);
        assert_eq!(tracker.claim_all(64), 12);
        let after_claim = tracker.snapshot();
        let (_, info_claim) = after_claim
            .relayers
            .iter()
            .find(|(id, _)| id == &relayer)
            .expect("relayer tracked");
        assert_eq!(after_claim.pending_total, 0);
        assert_eq!(info_claim.pending, 0);
        assert_eq!(info_claim.total_claimed, 12);
        assert_eq!(info_claim.last_claim_height, Some(64));

        let restored = tracker.rollback_claim(64);
        assert_eq!(restored, 12);
        let after_rollback = tracker.snapshot();
        assert_eq!(after_rollback.pending_total, 12);
        let (_, info_reverted) = after_rollback
            .relayers
            .iter()
            .find(|(id, _)| id == &relayer)
            .expect("relayer retained");
        assert_eq!(info_reverted.pending, 12);
        assert_eq!(info_reverted.total_claimed, 0);
        assert_eq!(info_reverted.last_claim_height, None);

        assert_eq!(tracker.claim_all(64), 12);
        let post_reclaim = tracker.snapshot();
        assert_eq!(post_reclaim.pending_total, 0);
        let (_, info_post) = post_reclaim
            .relayers
            .iter()
            .find(|(id, _)| id == &relayer)
            .expect("relayer retained");
        assert_eq!(info_post.pending, 0);
        assert_eq!(info_post.total_claimed, 12);
        assert_eq!(info_post.last_claim_height, Some(64));
    }

    #[test]
    fn zero_amount_claims_create_receipts() {
        let mut tracker = tracker_with_tempdir();
        assert_eq!(tracker.pending_total(), 0);
        // First claim at zero height should create a receipt preventing duplicates.
        assert_eq!(tracker.claim_all(5), 0);
        assert_eq!(tracker.claim_all(5), 0);
        // Rolling back removes the empty receipt so future claims can proceed.
        assert_eq!(tracker.rollback_claim(5), 0);
        assert_eq!(tracker.claim_all(5), 0);
    }

    #[test]
    fn receipt_history_orders_and_filters() {
        let mut tracker = tracker_with_tempdir();
        let relayer_a = b"alpha";
        let relayer_b = b"beta";
        tracker.record(relayer_a, 2, 10);
        tracker.record(relayer_b, 1, 5);
        tracker.claim_all(10);
        tracker.record(relayer_a, 1, 3);
        tracker.claim_all(15);

        let page = tracker.receipt_history(None, None, 10);
        assert_eq!(page.receipts.len(), 2);
        assert_eq!(page.receipts[0].height, 15);
        assert_eq!(page.receipts[0].amount, 3);
        assert_eq!(page.receipts[0].relayers.len(), 1);
        assert_eq!(page.receipts[0].relayers[0].id, relayer_a.to_vec());
        assert_eq!(page.receipts[0].relayers[0].amount, 3);
        assert_eq!(page.receipts[1].height, 10);
        assert_eq!(page.receipts[1].amount, 15);
        assert_eq!(page.receipts[1].relayers.len(), 2);

        let filtered = tracker.receipt_history(Some(relayer_b.as_slice()), None, 10);
        assert_eq!(filtered.receipts.len(), 1);
        assert_eq!(filtered.receipts[0].height, 10);
        assert_eq!(filtered.receipts[0].amount, 5);
        assert_eq!(filtered.receipts[0].relayers.len(), 1);
        assert_eq!(filtered.receipts[0].relayers[0].id, relayer_b.to_vec());

        let first_page = tracker.receipt_history(None, None, 1);
        assert_eq!(first_page.receipts.len(), 1);
        assert_eq!(first_page.receipts[0].height, 15);
        let next_cursor = first_page.next.expect("cursor");
        let second_page = tracker.receipt_history(None, Some(next_cursor), 1);
        assert_eq!(second_page.receipts.len(), 1);
        assert_eq!(second_page.receipts[0].height, 10);
    }
}

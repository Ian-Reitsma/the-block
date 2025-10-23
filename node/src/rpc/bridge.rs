use super::RpcError;
use crate::{
    bridge::{
        Bridge, BridgeError, ChallengeRecord, ChannelConfig, DepositReceipt, DisputeAuditRecord,
        DutyOutcomeSnapshot, PendingWithdrawalInfo, RelayerInfo, RelayerQuorumInfo,
        RewardAccrualRecord, RewardClaimRecord, SettlementRecord, SlashRecord,
    },
    simple_db::names,
    SimpleDb,
};
use bridge_types::{DutyKind, DutyRecord, DutyStatus, ExternalSettlementProof};
use bridges::{
    header::PowHeader, light_client::Proof, token_bridge::AssetSnapshot as BridgeAssetSnapshot,
    RelayerBundle, RelayerProof,
};
use concurrency::Lazy;
use foundation_serialization::{Deserialize, Serialize};
use ledger::Emission;
use std::convert::TryFrom;
use std::sync::{Mutex, MutexGuard};

static SERVICE: Lazy<Mutex<Bridge>> = Lazy::new(|| {
    let path = std::env::var("TB_BRIDGE_DB_PATH").unwrap_or_else(|_| "state/bridge_db".into());
    let db = SimpleDb::open_named(names::RPC_BRIDGE, &path);
    Mutex::new(Bridge::with_db(db))
});

fn guard() -> Result<MutexGuard<'static, Bridge>, RpcError> {
    SERVICE
        .lock()
        .map_err(|_| RpcError::new(-32000, "bridge busy"))
}

fn convert_err(err: BridgeError) -> RpcError {
    let (code, message) = match err {
        BridgeError::InvalidProof => (-32002, "invalid proof"),
        BridgeError::Replay => (-32006, "proof replay"),
        BridgeError::DuplicateWithdrawal => (-32007, "withdrawal already pending"),
        BridgeError::WithdrawalMissing => (-32008, "withdrawal not found"),
        BridgeError::AlreadyChallenged => (-32009, "withdrawal already challenged"),
        BridgeError::ChallengeWindowOpen => (-32010, "challenge window open"),
        BridgeError::UnauthorizedRelease => (-32011, "release not authorized"),
        BridgeError::UnknownChannel(_) => (-32012, "unknown bridge channel"),
        BridgeError::Storage(_) => (-32013, "bridge storage failure"),
        BridgeError::InsufficientBond { .. } => (-32014, "insufficient bond"),
        BridgeError::RewardClaimRejected(_) => (-32015, "reward claim rejected"),
        BridgeError::RewardClaimAmountZero => (-32016, "reward claim amount zero"),
        BridgeError::RewardInsufficientPending { .. } => (-32017, "insufficient pending reward"),
        BridgeError::SettlementProofRequired { .. } => (-32018, "settlement proof required"),
        BridgeError::SettlementProofDuplicate => (-32019, "settlement proof duplicate"),
        BridgeError::SettlementProofChainMismatch { .. } => {
            (-32020, "settlement proof chain mismatch")
        }
        BridgeError::SettlementProofNotTracked { .. } => (-32021, "settlement proof not tracked"),
        BridgeError::SettlementProofHashMismatch { .. } => {
            (-32022, "settlement proof hash mismatch")
        }
        BridgeError::SettlementProofHeightReplay { .. } => {
            (-32023, "settlement proof height replay")
        }
    };
    RpcError::new(code, message)
}

fn decode_hex32(hex: &str, error: &str) -> Result<[u8; 32], RpcError> {
    let bytes =
        crypto_suite::hex::decode(hex).map_err(|_| RpcError::new(-32602, error.to_string()))?;
    if bytes.len() != 32 {
        return Err(RpcError::new(-32602, error.to_string()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn decode_commitment(hex: &str) -> Result<[u8; 32], RpcError> {
    decode_hex32(hex, "invalid commitment")
}

fn decode_proof_hash(hex: &str) -> Result<[u8; 32], RpcError> {
    decode_hex32(hex, "invalid proof hash")
}

fn encode_hex(bytes: &[u8]) -> String {
    crypto_suite::hex::encode(bytes)
}

#[allow(dead_code)]
fn default_asset() -> String {
    "native".to_string()
}

#[allow(dead_code)]
fn default_limit() -> u64 {
    100
}

const REWARD_CLAIM_PAGE_MAX: usize = 256;
const REWARD_ACCRUAL_PAGE_MAX: usize = 256;
const SETTLEMENT_PAGE_MAX: usize = 256;
const DISPUTE_PAGE_MAX: usize = 256;

fn clamp_page_limit(limit: u64, max: usize) -> usize {
    let requested = usize::try_from(limit).unwrap_or(max);
    let clamped = requested.min(max);
    if clamped == 0 {
        1
    } else {
        clamped
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_page_limit;

    #[test]
    fn clamp_page_limit_enforces_bounds() {
        assert_eq!(clamp_page_limit(0, 256), 1);
        assert_eq!(clamp_page_limit(25, 256), 25);
        assert_eq!(clamp_page_limit(1024, 256), 256);
        assert_eq!(clamp_page_limit(u64::MAX, 256), 256);
    }
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerStatusRequest {
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub relayer: String,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BondRelayerRequest {
    #[serde(default)]
    pub relayer: String,
    #[serde(default)]
    pub amount: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ClaimRewardsRequest {
    #[serde(default)]
    pub relayer: String,
    #[serde(default)]
    pub amount: u64,
    #[serde(default)]
    pub approval_key: String,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct VerifyDepositRequest {
    #[serde(default = "default_asset")]
    pub asset: String,
    #[serde(default)]
    pub relayer: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub amount: u64,
    pub header: PowHeader,
    pub proof: Proof,
    #[serde(rename = "relayer_proofs")]
    pub relayer_proofs: Vec<RelayerProof>,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RequestWithdrawalRequest {
    #[serde(default = "default_asset")]
    pub asset: String,
    #[serde(default)]
    pub relayer: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub amount: u64,
    #[serde(rename = "relayer_proofs")]
    pub relayer_proofs: Vec<RelayerProof>,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ChallengeWithdrawalRequest {
    #[serde(default = "default_asset")]
    pub asset: String,
    #[serde(default)]
    pub commitment: String,
    #[serde(default)]
    pub challenger: String,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct FinalizeWithdrawalRequest {
    #[serde(default = "default_asset")]
    pub asset: String,
    #[serde(default)]
    pub commitment: String,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PendingWithdrawalsRequest {
    #[serde(default)]
    pub asset: Option<String>,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ActiveChallengesRequest {
    #[serde(default)]
    pub asset: Option<String>,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SubmitSettlementRequest {
    #[serde(default = "default_asset")]
    pub asset: String,
    #[serde(default)]
    pub relayer: String,
    #[serde(default)]
    pub commitment: String,
    #[serde(default)]
    pub settlement_chain: String,
    #[serde(default)]
    pub proof_hash: String,
    #[serde(default)]
    pub settlement_height: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerQuorumRequest {
    pub asset: String,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerAccountingRequest {
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub relayer: Option<String>,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DutyLogRequest {
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub relayer: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DepositHistoryRequest {
    pub asset: String,
    #[serde(default)]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SlashLogRequest {}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardClaimsRequest {
    #[serde(default)]
    pub relayer: Option<String>,
    #[serde(default)]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardAccrualsRequest {
    #[serde(default)]
    pub relayer: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SettlementLogRequest {
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisputeAuditRequest {
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AssetsRequest {}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ConfigureAssetRequest {
    #[serde(default = "default_asset")]
    pub asset: String,
    #[serde(default)]
    pub confirm_depth: Option<u64>,
    #[serde(default)]
    pub fee_per_byte: Option<u64>,
    #[serde(default)]
    pub challenge_period_secs: Option<u64>,
    #[serde(default)]
    pub relayer_quorum: Option<usize>,
    #[serde(default)]
    pub headers_dir: Option<String>,
    #[serde(default)]
    pub requires_settlement_proof: Option<bool>,
    #[serde(default)]
    pub settlement_chain: Option<String>,
    #[serde(default)]
    pub clear_settlement_chain: bool,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerStatusResponse {
    pub asset: String,
    pub relayer: String,
    pub stake: u64,
    pub slashes: u64,
    pub bond: u64,
    pub duties_assigned: u64,
    pub duties_completed: u64,
    pub duties_failed: u64,
    pub rewards_earned: u64,
    pub rewards_pending: u64,
    pub rewards_claimed: u64,
    pub penalties_applied: u64,
    pub pending_duties: u64,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct StatusResponse {
    pub status: &'static str,
}

impl StatusResponse {
    fn ok() -> Self {
        StatusResponse { status: "ok" }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct VerifyDepositResponse {
    pub status: &'static str,
    pub nonce: u64,
    pub commitment: String,
    pub recorded_at: u64,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct WithdrawalResponse {
    pub status: &'static str,
    pub commitment: String,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ChallengeWithdrawalResponse {
    pub status: &'static str,
    pub challenger: String,
    pub timestamp: u64,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct FinalizeWithdrawalResponse {
    pub status: &'static str,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PendingWithdrawalEntry {
    pub asset: String,
    pub commitment: String,
    pub user: String,
    pub amount: u64,
    pub relayers: Vec<String>,
    pub initiated_at: u64,
    pub deadline: u64,
    pub challenged: bool,
    pub requires_settlement_proof: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_submitted_at: Option<u64>,
}

impl From<PendingWithdrawalInfo> for PendingWithdrawalEntry {
    fn from(info: PendingWithdrawalInfo) -> Self {
        PendingWithdrawalEntry {
            asset: info.asset,
            commitment: encode_hex(&info.commitment),
            user: info.user,
            amount: info.amount,
            relayers: info.relayers,
            initiated_at: info.initiated_at,
            deadline: info.deadline,
            challenged: info.challenged,
            requires_settlement_proof: info.requires_settlement_proof,
            settlement_chain: info.settlement_chain,
            settlement_submitted_at: info.settlement_submitted_at,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PendingWithdrawalsResponse {
    pub withdrawals: Vec<PendingWithdrawalEntry>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ActiveChallengeEntry {
    pub asset: String,
    pub commitment: String,
    pub challenger: String,
    pub timestamp: u64,
}

impl From<ChallengeRecord> for ActiveChallengeEntry {
    fn from(record: ChallengeRecord) -> Self {
        ActiveChallengeEntry {
            asset: record.asset,
            commitment: encode_hex(&record.commitment),
            challenger: record.challenger,
            timestamp: record.challenged_at,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ActiveChallengesResponse {
    pub challenges: Vec<ActiveChallengeEntry>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerParticipant {
    pub id: String,
    pub stake: u64,
    pub slashes: u64,
    pub bond: u64,
    pub duties_assigned: u64,
    pub duties_completed: u64,
    pub duties_failed: u64,
    pub rewards_earned: u64,
    pub rewards_pending: u64,
    pub rewards_claimed: u64,
    pub penalties_applied: u64,
    pub pending_duties: u64,
}

impl From<RelayerInfo> for RelayerParticipant {
    fn from(info: RelayerInfo) -> Self {
        RelayerParticipant {
            id: info.id,
            stake: info.stake,
            slashes: info.slashes,
            bond: info.bond,
            duties_assigned: info.duties_assigned,
            duties_completed: info.duties_completed,
            duties_failed: info.duties_failed,
            rewards_earned: info.rewards_earned,
            rewards_pending: info.rewards_pending,
            rewards_claimed: info.rewards_claimed,
            penalties_applied: info.penalties_applied,
            pending_duties: info.pending_duties as u64,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerQuorumResponse {
    pub asset: String,
    pub quorum: u64,
    pub relayers: Vec<RelayerParticipant>,
}

impl From<RelayerQuorumInfo> for RelayerQuorumResponse {
    fn from(info: RelayerQuorumInfo) -> Self {
        RelayerQuorumResponse {
            asset: info.asset,
            quorum: info.quorum,
            relayers: info
                .relayers
                .into_iter()
                .map(RelayerParticipant::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DepositReceiptEntry {
    pub asset: String,
    pub nonce: u64,
    pub user: String,
    pub amount: u64,
    pub relayer: String,
    pub header_hash: String,
    pub commitment: String,
    pub fingerprint: String,
    pub relayers: Vec<String>,
    pub recorded_at: u64,
}

impl From<DepositReceipt> for DepositReceiptEntry {
    fn from(receipt: DepositReceipt) -> Self {
        DepositReceiptEntry {
            asset: receipt.asset,
            nonce: receipt.nonce,
            user: receipt.user,
            amount: receipt.amount,
            relayer: receipt.relayer,
            header_hash: encode_hex(&receipt.header_hash),
            commitment: encode_hex(&receipt.relayer_commitment),
            fingerprint: encode_hex(&receipt.proof_fingerprint),
            relayers: receipt.bundle_relayers,
            recorded_at: receipt.recorded_at,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DepositHistoryResponse {
    pub receipts: Vec<DepositReceiptEntry>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SlashLogEntry {
    pub relayer: String,
    pub asset: String,
    pub slashes: u64,
    pub remaining_bond: u64,
    pub timestamp: u64,
}

impl From<&SlashRecord> for SlashLogEntry {
    fn from(record: &SlashRecord) -> Self {
        SlashLogEntry {
            relayer: record.relayer.clone(),
            asset: record.asset.clone(),
            slashes: record.slashes,
            remaining_bond: record.remaining_bond,
            timestamp: record.occurred_at,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SlashLogResponse {
    pub slashes: Vec<SlashLogEntry>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerAccountingEntry {
    pub asset: String,
    pub relayer: String,
    pub stake: u64,
    pub slashes: u64,
    pub bond: u64,
    pub duties_assigned: u64,
    pub duties_completed: u64,
    pub duties_failed: u64,
    pub rewards_earned: u64,
    pub rewards_pending: u64,
    pub rewards_claimed: u64,
    pub penalties_applied: u64,
    pub pending_duties: u64,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerAccountingResponse {
    pub relayers: Vec<RelayerAccountingEntry>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardClaimEntry {
    pub id: u64,
    pub relayer: String,
    pub amount: u64,
    pub approval_key: String,
    pub claimed_at: u64,
    pub pending_before: u64,
    pub pending_after: u64,
}

impl From<RewardClaimRecord> for RewardClaimEntry {
    fn from(record: RewardClaimRecord) -> Self {
        RewardClaimEntry {
            id: record.id,
            relayer: record.relayer,
            amount: record.amount,
            approval_key: record.approval_key,
            claimed_at: record.claimed_at,
            pending_before: record.pending_before,
            pending_after: record.pending_after,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardClaimResponse {
    pub status: &'static str,
    pub claim: RewardClaimEntry,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardClaimsResponse {
    pub claims: Vec<RewardClaimEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardAccrualEntry {
    pub id: u64,
    pub relayer: String,
    pub asset: String,
    pub user: String,
    pub amount: u64,
    pub duty_id: u64,
    pub duty_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commitment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_hash: Option<String>,
    pub bundle_relayers: Vec<String>,
    pub recorded_at: u64,
}

impl From<RewardAccrualRecord> for RewardAccrualEntry {
    fn from(record: RewardAccrualRecord) -> Self {
        RewardAccrualEntry {
            id: record.id,
            relayer: record.relayer,
            asset: record.asset,
            user: record.user,
            amount: record.amount,
            duty_id: record.duty_id,
            duty_kind: record.duty_kind,
            commitment: record.commitment.map(|value| encode_hex(&value)),
            settlement_chain: record.settlement_chain,
            proof_hash: record.proof_hash.map(|value| encode_hex(&value)),
            bundle_relayers: record.bundle_relayers,
            recorded_at: record.recorded_at,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RewardAccrualsResponse {
    pub accruals: Vec<RewardAccrualEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SettlementEntry {
    pub asset: String,
    pub commitment: String,
    pub relayer: String,
    pub settlement_chain: Option<String>,
    pub proof_hash: String,
    pub settlement_height: u64,
    pub submitted_at: u64,
}

impl From<SettlementRecord> for SettlementEntry {
    fn from(record: SettlementRecord) -> Self {
        SettlementEntry {
            asset: record.asset,
            commitment: encode_hex(&record.commitment),
            relayer: record.relayer,
            settlement_chain: record.settlement_chain,
            proof_hash: encode_hex(&record.proof_hash),
            settlement_height: record.settlement_height,
            submitted_at: record.submitted_at,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SettlementResponse {
    pub status: &'static str,
    pub settlement: SettlementEntry,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SettlementLogResponse {
    pub settlements: Vec<SettlementEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DutyOutcomeSnapshotEntry {
    pub relayer: String,
    pub status: String,
    pub reward: u64,
    pub penalty: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub duty_id: u64,
}

impl From<DutyOutcomeSnapshot> for DutyOutcomeSnapshotEntry {
    fn from(snapshot: DutyOutcomeSnapshot) -> Self {
        DutyOutcomeSnapshotEntry {
            relayer: snapshot.relayer,
            status: snapshot.status,
            reward: snapshot.reward,
            penalty: snapshot.penalty,
            completed_at: snapshot.completed_at,
            failed_at: snapshot.failed_at,
            reason: snapshot.reason,
            duty_id: snapshot.duty_id,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisputeAuditEntry {
    pub asset: String,
    pub commitment: String,
    pub user: String,
    pub amount: u64,
    pub initiated_at: u64,
    pub deadline: u64,
    pub challenged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenged_at: Option<u64>,
    pub settlement_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_submitted_at: Option<u64>,
    pub relayer_outcomes: Vec<DutyOutcomeSnapshotEntry>,
    pub expired: bool,
}

impl From<DisputeAuditRecord> for DisputeAuditEntry {
    fn from(record: DisputeAuditRecord) -> Self {
        DisputeAuditEntry {
            asset: record.asset,
            commitment: encode_hex(&record.commitment),
            user: record.user,
            amount: record.amount,
            initiated_at: record.initiated_at,
            deadline: record.deadline,
            challenged: record.challenged,
            challenger: record.challenger,
            challenged_at: record.challenged_at,
            settlement_required: record.settlement_required,
            settlement_chain: record.settlement_chain,
            settlement_submitted_at: record.settlement_submitted_at,
            relayer_outcomes: record
                .relayer_outcomes
                .into_iter()
                .map(DutyOutcomeSnapshotEntry::from)
                .collect(),
            expired: record.expired,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisputeAuditResponse {
    pub disputes: Vec<DisputeAuditEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EmissionEntry {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate: Option<u64>,
}

impl From<&Emission> for EmissionEntry {
    fn from(emission: &Emission) -> Self {
        match emission {
            Emission::Fixed(amount) => Self {
                kind: "fixed",
                amount: Some(*amount),
                initial: None,
                rate: None,
            },
            Emission::Linear { initial, rate } => Self {
                kind: "linear",
                amount: None,
                initial: Some(*initial),
                rate: Some(*rate),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AssetEntry {
    pub symbol: String,
    pub locked: u64,
    pub minted: u64,
    pub emission: EmissionEntry,
}

impl From<BridgeAssetSnapshot> for AssetEntry {
    fn from(snapshot: BridgeAssetSnapshot) -> Self {
        AssetEntry {
            emission: EmissionEntry::from(&snapshot.emission),
            locked: snapshot.locked,
            minted: snapshot.minted,
            symbol: snapshot.symbol,
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AssetsResponse {
    pub assets: Vec<AssetEntry>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ConfigureAssetResponse {
    pub status: &'static str,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DutyStatusEntry {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penalty: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DutyLogEntry {
    pub id: u64,
    pub asset: String,
    pub relayer: String,
    pub user: String,
    pub amount: u64,
    pub assigned_at: u64,
    pub deadline: u64,
    pub bundle_relayers: Vec<String>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commitment: Option<String>,
    pub status: DutyStatusEntry,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DutyLogResponse {
    pub duties: Vec<DutyLogEntry>,
}

impl From<&DutyStatus> for DutyStatusEntry {
    fn from(status: &DutyStatus) -> Self {
        match status {
            DutyStatus::Pending => DutyStatusEntry {
                status: "pending".into(),
                reward: None,
                completed_at: None,
                penalty: None,
                failed_at: None,
                reason: None,
            },
            DutyStatus::Completed {
                reward,
                completed_at,
            } => DutyStatusEntry {
                status: "completed".into(),
                reward: Some(*reward),
                completed_at: Some(*completed_at),
                penalty: None,
                failed_at: None,
                reason: None,
            },
            DutyStatus::Failed {
                penalty,
                failed_at,
                reason,
            } => DutyStatusEntry {
                status: "failed".into(),
                reward: None,
                completed_at: None,
                penalty: Some(*penalty),
                failed_at: Some(*failed_at),
                reason: Some(reason.as_str().to_string()),
            },
        }
    }
}

impl From<DutyRecord> for DutyLogEntry {
    fn from(record: DutyRecord) -> Self {
        let (kind, commitment) = match &record.kind {
            DutyKind::Deposit => ("deposit".to_string(), None),
            DutyKind::Withdrawal { commitment } => {
                ("withdrawal".to_string(), Some(encode_hex(commitment)))
            }
            DutyKind::Settlement { commitment, .. } => {
                ("settlement".to_string(), Some(encode_hex(commitment)))
            }
        };
        DutyLogEntry {
            id: record.id,
            asset: record.asset,
            relayer: record.relayer,
            user: record.user,
            amount: record.amount,
            assigned_at: record.assigned_at,
            deadline: record.deadline,
            bundle_relayers: record.bundle_relayers,
            kind,
            commitment,
            status: DutyStatusEntry::from(&record.status),
        }
    }
}

pub fn relayer_status(req: RelayerStatusRequest) -> Result<RelayerStatusResponse, RpcError> {
    let RelayerStatusRequest { asset, relayer } = req;
    let asset_hint = asset.clone().unwrap_or_default();
    let relayer_id = relayer.clone();
    let bridge = guard()?;
    let result = bridge.relayer_status(&relayer, asset.as_deref());
    drop(bridge);
    if let Some((asset_id, info)) = result {
        Ok(RelayerStatusResponse {
            asset: asset_id,
            relayer,
            stake: info.stake,
            slashes: info.slashes,
            bond: info.bond,
            duties_assigned: info.duties_assigned,
            duties_completed: info.duties_completed,
            duties_failed: info.duties_failed,
            rewards_earned: info.rewards_earned,
            rewards_pending: info.rewards_pending,
            rewards_claimed: info.rewards_claimed,
            penalties_applied: info.penalties_applied,
            pending_duties: info.pending_duties as u64,
        })
    } else {
        Ok(RelayerStatusResponse {
            asset: asset_hint,
            relayer: relayer_id,
            stake: 0,
            slashes: 0,
            bond: 0,
            duties_assigned: 0,
            duties_completed: 0,
            duties_failed: 0,
            rewards_earned: 0,
            rewards_pending: 0,
            rewards_claimed: 0,
            penalties_applied: 0,
            pending_duties: 0,
        })
    }
}

pub fn bond_relayer(req: BondRelayerRequest) -> Result<StatusResponse, RpcError> {
    let BondRelayerRequest { relayer, amount } = req;
    let mut bridge = guard()?;
    bridge.bond_relayer(&relayer, amount).map_err(convert_err)?;
    Ok(StatusResponse::ok())
}

pub fn claim_rewards(req: ClaimRewardsRequest) -> Result<RewardClaimResponse, RpcError> {
    let ClaimRewardsRequest {
        relayer,
        amount,
        approval_key,
    } = req;
    let mut bridge = guard()?;
    let record = bridge
        .claim_rewards(&relayer, amount, &approval_key)
        .map_err(convert_err)?;
    Ok(RewardClaimResponse {
        status: "claimed",
        claim: RewardClaimEntry::from(record),
    })
}

pub fn verify_deposit(req: VerifyDepositRequest) -> Result<VerifyDepositResponse, RpcError> {
    if req.relayer_proofs.is_empty() {
        return Err(RpcError::new(-32602, "no relayer proofs"));
    }
    let VerifyDepositRequest {
        asset,
        relayer,
        user,
        amount,
        header,
        proof,
        relayer_proofs,
    } = req;
    let bundle = RelayerBundle::new(relayer_proofs);
    let mut bridge = guard()?;
    let receipt = bridge
        .deposit(&asset, &relayer, &user, amount, &header, &proof, &bundle)
        .map_err(convert_err)?;
    Ok(VerifyDepositResponse {
        status: "ok",
        nonce: receipt.nonce,
        commitment: encode_hex(&receipt.relayer_commitment),
        recorded_at: receipt.recorded_at,
    })
}

pub fn request_withdrawal(req: RequestWithdrawalRequest) -> Result<WithdrawalResponse, RpcError> {
    if req.relayer_proofs.is_empty() {
        return Err(RpcError::new(-32602, "no relayer proofs"));
    }
    let RequestWithdrawalRequest {
        asset,
        relayer,
        user,
        amount,
        relayer_proofs,
    } = req;
    let bundle = RelayerBundle::new(relayer_proofs);
    let mut bridge = guard()?;
    let commitment = bridge
        .request_withdrawal(&asset, &relayer, &user, amount, &bundle)
        .map_err(convert_err)?;
    Ok(WithdrawalResponse {
        status: "pending",
        commitment: encode_hex(&commitment),
    })
}

pub fn challenge_withdrawal(
    req: ChallengeWithdrawalRequest,
) -> Result<ChallengeWithdrawalResponse, RpcError> {
    let ChallengeWithdrawalRequest {
        asset,
        commitment,
        challenger,
    } = req;
    let key = decode_commitment(&commitment)?;
    let mut bridge = guard()?;
    let record = bridge
        .challenge_withdrawal(&asset, key, &challenger)
        .map_err(convert_err)?;
    Ok(ChallengeWithdrawalResponse {
        status: "challenged",
        challenger: record.challenger,
        timestamp: record.challenged_at,
    })
}

pub fn finalize_withdrawal(
    req: FinalizeWithdrawalRequest,
) -> Result<FinalizeWithdrawalResponse, RpcError> {
    let FinalizeWithdrawalRequest { asset, commitment } = req;
    let key = decode_commitment(&commitment)?;
    let mut bridge = guard()?;
    bridge
        .finalize_withdrawal(&asset, key)
        .map_err(convert_err)?;
    Ok(FinalizeWithdrawalResponse {
        status: "finalized",
    })
}

pub fn submit_settlement(req: SubmitSettlementRequest) -> Result<SettlementResponse, RpcError> {
    let SubmitSettlementRequest {
        asset,
        relayer,
        commitment,
        settlement_chain,
        proof_hash,
        settlement_height,
    } = req;
    let commitment = decode_commitment(&commitment)?;
    let proof_hash = decode_proof_hash(&proof_hash)?;
    let proof = ExternalSettlementProof {
        commitment,
        settlement_chain,
        proof_hash,
        settlement_height,
    };
    let mut bridge = guard()?;
    let record = bridge
        .submit_settlement_proof(&asset, &relayer, proof)
        .map_err(convert_err)?;
    Ok(SettlementResponse {
        status: "submitted",
        settlement: SettlementEntry::from(record),
    })
}

pub fn pending_withdrawals(
    req: PendingWithdrawalsRequest,
) -> Result<PendingWithdrawalsResponse, RpcError> {
    let bridge = guard()?;
    let withdrawals = bridge
        .pending_withdrawals(req.asset.as_deref())
        .into_iter()
        .map(PendingWithdrawalEntry::from)
        .collect();
    Ok(PendingWithdrawalsResponse { withdrawals })
}

pub fn active_challenges(
    req: ActiveChallengesRequest,
) -> Result<ActiveChallengesResponse, RpcError> {
    let bridge = guard()?;
    let challenges = bridge
        .challenges(req.asset.as_deref())
        .into_iter()
        .map(ActiveChallengeEntry::from)
        .collect();
    Ok(ActiveChallengesResponse { challenges })
}

pub fn relayer_quorum(req: RelayerQuorumRequest) -> Result<RelayerQuorumResponse, RpcError> {
    let bridge = guard()?;
    let info = bridge
        .relayer_quorum(&req.asset)
        .ok_or_else(|| RpcError::new(-32012, "unknown bridge channel"))?;
    Ok(RelayerQuorumResponse::from(info))
}

pub fn reward_claims(req: RewardClaimsRequest) -> Result<RewardClaimsResponse, RpcError> {
    let bridge = guard()?;
    let limit = clamp_page_limit(req.limit, REWARD_CLAIM_PAGE_MAX);
    let (records, next_cursor) = bridge.reward_claims(req.relayer.as_deref(), req.cursor, limit);
    let claims = records.into_iter().map(RewardClaimEntry::from).collect();
    Ok(RewardClaimsResponse {
        claims,
        next_cursor,
    })
}

pub fn reward_accruals(req: RewardAccrualsRequest) -> Result<RewardAccrualsResponse, RpcError> {
    let bridge = guard()?;
    let limit = clamp_page_limit(req.limit, REWARD_ACCRUAL_PAGE_MAX);
    let (records, next_cursor) = bridge.reward_accruals(
        req.relayer.as_deref(),
        req.asset.as_deref(),
        req.cursor,
        limit,
    );
    let accruals = records.into_iter().map(RewardAccrualEntry::from).collect();
    Ok(RewardAccrualsResponse {
        accruals,
        next_cursor,
    })
}

pub fn settlement_log(req: SettlementLogRequest) -> Result<SettlementLogResponse, RpcError> {
    let bridge = guard()?;
    let limit = clamp_page_limit(req.limit, SETTLEMENT_PAGE_MAX);
    let (records, next_cursor) = bridge.settlement_records(req.asset.as_deref(), req.cursor, limit);
    let settlements = records.into_iter().map(SettlementEntry::from).collect();
    Ok(SettlementLogResponse {
        settlements,
        next_cursor,
    })
}

pub fn dispute_audit(req: DisputeAuditRequest) -> Result<DisputeAuditResponse, RpcError> {
    let bridge = guard()?;
    let limit = clamp_page_limit(req.limit, DISPUTE_PAGE_MAX);
    let (records, next_cursor) = bridge.dispute_audit(req.asset.as_deref(), req.cursor, limit);
    let disputes = records.into_iter().map(DisputeAuditEntry::from).collect();
    Ok(DisputeAuditResponse {
        disputes,
        next_cursor,
    })
}

pub fn relayer_accounting(
    req: RelayerAccountingRequest,
) -> Result<RelayerAccountingResponse, RpcError> {
    let RelayerAccountingRequest { asset, relayer } = req;
    let bridge = guard()?;
    let relayers = bridge
        .relayer_accounting(relayer.as_deref(), asset.as_deref())
        .into_iter()
        .map(|(asset, info)| RelayerAccountingEntry {
            asset,
            relayer: info.id,
            stake: info.stake,
            slashes: info.slashes,
            bond: info.bond,
            duties_assigned: info.duties_assigned,
            duties_completed: info.duties_completed,
            duties_failed: info.duties_failed,
            rewards_earned: info.rewards_earned,
            rewards_pending: info.rewards_pending,
            rewards_claimed: info.rewards_claimed,
            penalties_applied: info.penalties_applied,
            pending_duties: info.pending_duties as u64,
        })
        .collect();
    Ok(RelayerAccountingResponse { relayers })
}

pub fn duty_log(req: DutyLogRequest) -> Result<DutyLogResponse, RpcError> {
    let DutyLogRequest {
        asset,
        relayer,
        limit,
    } = req;
    let bridge = guard()?;
    let duties = bridge
        .duty_log(relayer.as_deref(), asset.as_deref(), limit as usize)
        .into_iter()
        .map(DutyLogEntry::from)
        .collect();
    Ok(DutyLogResponse { duties })
}

pub fn deposit_history(req: DepositHistoryRequest) -> Result<DepositHistoryResponse, RpcError> {
    let limit = req.limit as usize;
    let bridge = guard()?;
    let receipts = bridge
        .deposit_history(&req.asset, req.cursor, limit)
        .into_iter()
        .map(DepositReceiptEntry::from)
        .collect();
    Ok(DepositHistoryResponse { receipts })
}

pub fn slash_log(_req: SlashLogRequest) -> Result<SlashLogResponse, RpcError> {
    let bridge = guard()?;
    let slashes = bridge.slash_log().iter().map(SlashLogEntry::from).collect();
    Ok(SlashLogResponse { slashes })
}

pub fn assets(_req: AssetsRequest) -> Result<AssetsResponse, RpcError> {
    let bridge = guard()?;
    let assets = bridge
        .asset_snapshots()
        .into_iter()
        .map(AssetEntry::from)
        .collect();
    Ok(AssetsResponse { assets })
}

pub fn configure_asset(req: ConfigureAssetRequest) -> Result<ConfigureAssetResponse, RpcError> {
    let mut bridge = guard()?;
    let ConfigureAssetRequest {
        asset,
        confirm_depth,
        fee_per_byte,
        challenge_period_secs,
        relayer_quorum,
        headers_dir,
        requires_settlement_proof,
        settlement_chain,
        clear_settlement_chain,
    } = req;
    let mut config = bridge
        .channel_config(&asset)
        .unwrap_or_else(|| ChannelConfig::for_asset(&asset));
    if let Some(depth) = confirm_depth {
        config.confirm_depth = depth;
    }
    if let Some(fee) = fee_per_byte {
        config.fee_per_byte = fee;
    }
    if let Some(period) = challenge_period_secs {
        config.challenge_period_secs = period;
    }
    if let Some(quorum) = relayer_quorum {
        config.relayer_quorum = quorum;
    }
    if let Some(dir) = headers_dir {
        config.headers_dir = dir;
    }
    if let Some(flag) = requires_settlement_proof {
        config.requires_settlement_proof = flag;
    }
    if clear_settlement_chain {
        config.settlement_chain = None;
    } else if let Some(chain) = settlement_chain {
        config.settlement_chain = Some(chain);
    }
    bridge
        .set_channel_config(&asset, config)
        .map_err(convert_err)?;
    Ok(ConfigureAssetResponse { status: "ok" })
}

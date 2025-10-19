use super::RpcError;
use crate::{
    bridge::{
        Bridge, BridgeError, ChallengeRecord, DepositReceipt, PendingWithdrawalInfo, RelayerInfo,
        RelayerQuorumInfo, SlashRecord,
    },
    simple_db::names,
    SimpleDb,
};
use bridges::{header::PowHeader, light_client::Proof, RelayerBundle, RelayerProof};
use concurrency::Lazy;
use foundation_serialization::{Deserialize, Serialize};
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
    };
    RpcError::new(code, message)
}

fn decode_commitment(hex: &str) -> Result<[u8; 32], RpcError> {
    let bytes =
        crypto_suite::hex::decode(hex).map_err(|_| RpcError::new(-32602, "invalid commitment"))?;
    if bytes.len() != 32 {
        return Err(RpcError::new(-32602, "invalid commitment"));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn encode_hex(bytes: &[u8]) -> String {
    crypto_suite::hex::encode(bytes)
}

fn default_asset() -> String {
    "native".to_string()
}

fn default_limit() -> u64 {
    100
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
pub struct RelayerQuorumRequest {
    pub asset: String,
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

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RelayerStatusResponse {
    pub asset: String,
    pub stake: u64,
    pub slashes: u64,
    pub bond: u64,
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
}

impl From<RelayerInfo> for RelayerParticipant {
    fn from(info: RelayerInfo) -> Self {
        RelayerParticipant {
            id: info.id,
            stake: info.stake,
            slashes: info.slashes,
            bond: info.bond,
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

pub fn relayer_status(req: RelayerStatusRequest) -> Result<RelayerStatusResponse, RpcError> {
    let RelayerStatusRequest { asset, relayer } = req;
    let asset_hint = asset.clone().unwrap_or_default();
    let bridge = guard()?;
    let result = bridge.relayer_status(&relayer, asset.as_deref());
    drop(bridge);
    if let Some((asset_id, stake, slashes, bond)) = result {
        Ok(RelayerStatusResponse {
            asset: asset_id,
            stake,
            slashes,
            bond,
        })
    } else {
        Ok(RelayerStatusResponse {
            asset: asset_hint,
            stake: 0,
            slashes: 0,
            bond: 0,
        })
    }
}

pub fn bond_relayer(req: BondRelayerRequest) -> Result<StatusResponse, RpcError> {
    let BondRelayerRequest { relayer, amount } = req;
    let mut bridge = guard()?;
    bridge.bond_relayer(&relayer, amount).map_err(convert_err)?;
    Ok(StatusResponse::ok())
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

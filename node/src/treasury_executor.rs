use crate::governance::{
    DisbursementStatus, GovStore, SignedExecutionIntent, TreasuryDisbursement,
    TreasuryExecutorConfig, TreasuryExecutorError, TreasuryExecutorHandle,
};
use crate::transaction::{binary, sign_tx, RawTxPayload};
use crate::{Account, Blockchain, TxAdmissionError, EPOCH_BLOCKS};
use crypto_suite::hex;
use foundation_serialization::json::{self, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

pub type DependencyCheck = Arc<
    dyn Fn(&GovStore, &TreasuryDisbursement) -> Result<bool, TreasuryExecutorError> + Send + Sync,
>;

pub struct ExecutorParams {
    pub identity: String,
    pub poll_interval: Duration,
    pub lease_ttl: Duration,
    pub signing_key: Arc<Vec<u8>>,
    pub treasury_account: String,
    pub dependency_check: Option<DependencyCheck>,
}

fn parse_dependency_list(memo: &str) -> Vec<u64> {
    let trimmed = memo.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if let Ok(Value::Object(map)) = json::from_str::<Value>(trimmed) {
        if let Some(Value::Array(items)) = map.get("depends_on") {
            return items
                .iter()
                .filter_map(|item| match item {
                    Value::Number(num) => num.as_u64(),
                    Value::String(text) => text.trim().parse::<u64>().ok(),
                    _ => None,
                })
                .collect();
        }
    }

    if let Some(rest) = trimmed
        .strip_prefix("depends_on=")
        .or_else(|| trimmed.strip_prefix("depends_on:"))
    {
        return rest
            .split(',')
            .filter_map(|entry| entry.trim().parse::<u64>().ok())
            .collect();
    }

    Vec::new()
}

fn dependencies_ready(
    store: &GovStore,
    disbursement: &TreasuryDisbursement,
) -> Result<bool, TreasuryExecutorError> {
    let dependencies = parse_dependency_list(&disbursement.memo);
    if dependencies.is_empty() {
        return Ok(true);
    }

    let known = store.disbursements()?;
    for dep_id in dependencies {
        let Some(record) = known.iter().find(|entry| entry.id == dep_id) else {
            return Err(TreasuryExecutorError::Storage(format!(
                "dependency {dep_id} missing for disbursement {}",
                disbursement.id
            )));
        };
        if matches!(record.status, DisbursementStatus::Scheduled) {
            return Ok(false);
        }
    }

    Ok(true)
}

pub fn memo_dependency_check() -> DependencyCheck {
    Arc::new(|store: &GovStore, disbursement: &TreasuryDisbursement| {
        dependencies_ready(store, disbursement)
    })
}

fn next_available_nonce(account: &Account) -> u64 {
    let mut candidate = account
        .nonce
        .saturating_add(account.pending_nonce)
        .saturating_add(1);
    while account.pending_nonces.contains(&candidate) {
        candidate = candidate.saturating_add(1);
    }
    candidate
}

fn compute_epoch(block_height: u64) -> u64 {
    block_height / EPOCH_BLOCKS
}

fn signer_closure(
    blockchain: Arc<Mutex<Blockchain>>,
    treasury_account: String,
    signing_key: Arc<Vec<u8>>,
    nonce_floor: Arc<AtomicU64>,
) -> Arc<
    dyn Fn(&TreasuryDisbursement) -> Result<SignedExecutionIntent, TreasuryExecutorError>
        + Send
        + Sync,
> {
    Arc::new(move |disbursement: &TreasuryDisbursement| {
        let (base_fee, nonce, min_fee_per_byte, available_consumer) = {
            let guard = blockchain
                .lock()
                .map_err(|_| TreasuryExecutorError::Storage("blockchain lock poisoned".into()))?;
            let account = guard
                .accounts
                .get(&treasury_account)
                .ok_or_else(|| TreasuryExecutorError::Signing("treasury account missing".into()))?;
            let available_consumer = account
                .balance
                .consumer
                .saturating_sub(account.pending_consumer);
            let available_industrial = account
                .balance
                .industrial
                .saturating_sub(account.pending_industrial);
            if available_industrial < disbursement.amount_it {
                return Err(TreasuryExecutorError::cancelled(
                    "insufficient treasury IT balance",
                ));
            }
            let candidate = next_available_nonce(account);
            let floor = nonce_floor.load(Ordering::SeqCst);
            (
                guard.base_fee,
                candidate.max(floor.saturating_add(1)),
                guard.min_fee_per_byte_consumer,
                available_consumer,
            )
        };

        let mut payload = RawTxPayload {
            from_: treasury_account.clone(),
            to: disbursement.destination.clone(),
            amount_consumer: disbursement.amount_ct,
            amount_industrial: disbursement.amount_it,
            fee: base_fee,
            pct_ct: 100,
            nonce,
            memo: disbursement.memo.as_bytes().to_vec(),
        };

        let (signed, tx_bytes) = loop {
            let candidate = sign_tx(&signing_key, &payload).ok_or_else(|| {
                TreasuryExecutorError::Signing("invalid treasury signing key".into())
            })?;
            let bytes = binary::encode_signed_transaction(&candidate)
                .map_err(|e| TreasuryExecutorError::Signing(format!("encode signed tx: {e}")))?;
            let required_fee =
                base_fee.saturating_add(min_fee_per_byte.saturating_mul(bytes.len() as u64));
            if payload.fee >= required_fee {
                break (candidate, bytes);
            }
            payload.fee = required_fee;
        };
        let total_consumer = disbursement
            .amount_ct
            .checked_add(payload.fee)
            .ok_or_else(|| {
                TreasuryExecutorError::Signing("treasury disbursement exceeds u64".into())
            })?;
        if available_consumer < total_consumer {
            return Err(TreasuryExecutorError::cancelled(
                "insufficient treasury CT balance",
            ));
        }
        let tx_hash = hex::encode(signed.id());
        Ok(SignedExecutionIntent::new(
            disbursement.id,
            tx_bytes,
            tx_hash,
            payload.nonce,
        ))
    })
}

fn submitter_closure(
    blockchain: Arc<Mutex<Blockchain>>,
) -> Arc<dyn Fn(&SignedExecutionIntent) -> Result<String, TreasuryExecutorError> + Send + Sync> {
    Arc::new(move |intent: &SignedExecutionIntent| {
        let tx = binary::decode_signed_transaction(&intent.tx_bytes)
            .map_err(|e| TreasuryExecutorError::Submission(format!("decode signed tx: {e}")))?;
        let mut guard = blockchain
            .lock()
            .map_err(|_| TreasuryExecutorError::Storage("blockchain lock poisoned".into()))?;
        match guard.submit_transaction(tx) {
            Ok(()) => Ok(intent.tx_hash.clone()),
            Err(err) => {
                let message = format!("{} (code {})", err, err.code());
                match err {
                    TxAdmissionError::UnknownSender
                    | TxAdmissionError::InsufficientBalance
                    | TxAdmissionError::InvalidSelector
                    | TxAdmissionError::BalanceOverflow
                    | TxAdmissionError::FeeOverflow
                    | TxAdmissionError::FeeTooLarge
                    | TxAdmissionError::FeeTooLow => Err(TreasuryExecutorError::cancelled(message)),
                    _ => Err(TreasuryExecutorError::Submission(message)),
                }
            }
        }
    })
}

fn epoch_source_closure(blockchain: Arc<Mutex<Blockchain>>) -> Arc<dyn Fn() -> u64 + Send + Sync> {
    Arc::new(move || {
        let guard = blockchain
            .lock()
            .map(|g| g)
            .unwrap_or_else(|poison| poison.into_inner());
        compute_epoch(guard.block_height)
    })
}

pub fn spawn_executor(
    store: &GovStore,
    blockchain: Arc<Mutex<Blockchain>>,
    params: ExecutorParams,
) -> TreasuryExecutorHandle {
    let ExecutorParams {
        identity,
        poll_interval,
        lease_ttl,
        signing_key,
        treasury_account,
        dependency_check,
    } = params;
    let epoch_source = epoch_source_closure(Arc::clone(&blockchain));
    let nonce_floor = Arc::new(AtomicU64::new(0));
    let signer = signer_closure(
        Arc::clone(&blockchain),
        treasury_account.clone(),
        Arc::clone(&signing_key),
        Arc::clone(&nonce_floor),
    );
    let submitter = submitter_closure(blockchain);
    let dependency_check = dependency_check.unwrap_or_else(memo_dependency_check);
    let config = TreasuryExecutorConfig {
        identity,
        poll_interval,
        lease_ttl,
        epoch_source,
        signer,
        submitter,
        dependency_check: Some(dependency_check),
        nonce_floor,
    };
    store.spawn_treasury_executor(config)
}

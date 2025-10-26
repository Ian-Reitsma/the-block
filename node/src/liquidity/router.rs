#![forbid(unsafe_code)]

use crate::bridge::{Bridge, BridgeError, PendingWithdrawalInfo};
use crate::dex::{storage::EscrowState, DexStore, OrderBook, TrustLedger};
use crypto_suite::hashing::blake3::Hasher;
use dex::escrow::EscrowId;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Configuration knobs exposed to governance for the shared liquidity router.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Maximum number of intents to execute per batch.
    pub batch_size: usize,
    /// Fairness window injected as deterministic jitter (anti-front-running).
    pub fairness_window: Duration,
    /// Maximum fallback depth searched when bridging trust-line gaps.
    pub max_trust_hops: usize,
    /// Ignore trust-line imbalances below this threshold.
    pub min_trust_rebalance: u64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            batch_size: 32,
            fairness_window: Duration::from_millis(250),
            max_trust_hops: 6,
            min_trust_rebalance: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub enum LiquidityIntent {
    DexEscrow {
        escrow_id: EscrowId,
        buy: crate::dex::Order,
        sell: crate::dex::Order,
        quantity: u64,
        locked_at: u64,
    },
    BridgeWithdrawal {
        asset: String,
        commitment: [u8; 32],
        user: String,
        amount: u64,
        deadline: u64,
    },
    TrustRebalance {
        path: Vec<String>,
        amount: u64,
    },
}

impl LiquidityIntent {
    fn fingerprint(&self) -> Vec<u8> {
        match self {
            LiquidityIntent::DexEscrow {
                escrow_id,
                buy,
                sell,
                quantity,
                locked_at,
            } => {
                let mut h = Hasher::new();
                h.update(&escrow_id.to_le_bytes());
                h.update(buy.account.as_bytes());
                h.update(sell.account.as_bytes());
                h.update(&buy.price.to_le_bytes());
                h.update(&sell.price.to_le_bytes());
                h.update(&quantity.to_le_bytes());
                h.update(&locked_at.to_le_bytes());
                h.finalize().as_bytes().to_vec()
            }
            LiquidityIntent::BridgeWithdrawal {
                asset,
                commitment,
                user,
                amount,
                deadline,
            } => {
                let mut h = Hasher::new();
                h.update(asset.as_bytes());
                h.update(user.as_bytes());
                h.update(commitment);
                h.update(&amount.to_le_bytes());
                h.update(&deadline.to_le_bytes());
                h.finalize().as_bytes().to_vec()
            }
            LiquidityIntent::TrustRebalance { path, amount } => {
                let mut h = Hasher::new();
                for hop in path {
                    h.update(hop.as_bytes());
                }
                h.update(&amount.to_le_bytes());
                h.finalize().as_bytes().to_vec()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SequencedIntent {
    pub intent: LiquidityIntent,
    priority: u128,
    slot: u64,
    tie_breaker: u64,
}

impl SequencedIntent {
    fn new(intent: LiquidityIntent, slot: u64, tie_breaker: u64) -> Self {
        let priority = ((slot as u128) << 64) | tie_breaker as u128;
        Self {
            intent,
            priority,
            slot,
            tie_breaker,
        }
    }

    pub fn slot(&self) -> u64 {
        self.slot
    }

    pub fn tie_breaker(&self) -> u64 {
        self.tie_breaker
    }
}

#[derive(Debug, Clone, Default)]
pub struct LiquidityBatch {
    planned_at: u64,
    entropy: [u8; 32],
    intents: Vec<SequencedIntent>,
}

impl LiquidityBatch {
    pub fn intents(&self) -> &[SequencedIntent] {
        &self.intents
    }

    pub fn entropy(&self) -> [u8; 32] {
        self.entropy
    }

    pub fn planned_at(&self) -> u64 {
        self.planned_at
    }

    pub fn is_empty(&self) -> bool {
        self.intents.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct LiquidityExecution {
    pub released_escrows: Vec<EscrowId>,
    pub finalized_withdrawals: Vec<(String, [u8; 32])>,
    pub trust_rebalances: Vec<(Vec<String>, u64)>,
}

#[derive(Debug)]
pub enum RouterError {
    EscrowMissing(EscrowId),
    Dex(&'static str),
    TrustPathUnavailable {
        from: String,
        to: String,
        amount: u64,
    },
    TrustSettlementFailed(Vec<String>),
    Bridge(BridgeError),
    ArithmeticOverflow,
}

impl From<BridgeError> for RouterError {
    fn from(value: BridgeError) -> Self {
        Self::Bridge(value)
    }
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::EscrowMissing(id) => write!(f, "escrow entry {id} missing from state"),
            RouterError::Dex(reason) => write!(f, "escrow settlement failed: {reason}"),
            RouterError::TrustPathUnavailable { from, to, amount } => {
                write!(f, "trust path unavailable from {from} to {to} for {amount}")
            }
            RouterError::TrustSettlementFailed(path) => {
                write!(f, "trust settlement failed for path {path:?}")
            }
            RouterError::Bridge(err) => write!(f, "bridge error: {err}"),
            RouterError::ArithmeticOverflow => {
                write!(f, "arithmetic overflow computing trade value")
            }
        }
    }
}

impl std::error::Error for RouterError {}

pub struct LiquidityRouter {
    config: RouterConfig,
}

impl LiquidityRouter {
    pub fn new(config: RouterConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    fn fairness_range(&self) -> u64 {
        const CLASS_SCALE: u128 = 1_000_000;
        let cap = CLASS_SCALE / 6;
        let window = self.config.fairness_window.as_micros();
        if window == 0 {
            0
        } else {
            window.min(cap) as u64
        }
    }

    fn compute_slot(
        &self,
        primary: u64,
        class_bias: u64,
        tie_breaker: u64,
        fairness_range: u64,
    ) -> u64 {
        const CLASS_SCALE: u64 = 1_000_000;
        let jitter = if fairness_range == 0 {
            tie_breaker % (CLASS_SCALE / 6 + 1)
        } else {
            tie_breaker % fairness_range
        };
        primary
            .saturating_mul(CLASS_SCALE)
            .saturating_add(class_bias)
            .saturating_add(jitter)
    }

    fn tie_break(entropy: [u8; 32], fingerprint: &[u8], ordinal: u64) -> u64 {
        let mut hasher = Hasher::new();
        hasher.update(&entropy);
        hasher.update(&ordinal.to_le_bytes());
        hasher.update(fingerprint);
        let bytes = hasher.finalize();
        let mut out = [0u8; 8];
        out.copy_from_slice(&bytes.as_bytes()[..8]);
        u64::from_le_bytes(out)
    }

    pub fn schedule(
        &self,
        order_book: &OrderBook,
        escrow: &EscrowState,
        withdrawals: &[PendingWithdrawalInfo],
        trust: &TrustLedger,
        entropy: [u8; 32],
        now: SystemTime,
    ) -> LiquidityBatch {
        let now_secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let fairness_range = self.fairness_range();
        let mut sequenced = Vec::new();
        let mut ordinal: u64 = (order_book.bids.len() + order_book.asks.len()) as u64;
        const BIAS_BRIDGE: u64 = 0;
        const BIAS_DEX: u64 = 250_000;
        const BIAS_TRUST: u64 = 500_000;

        for (escrow_id, (buy, sell, qty, locked_at)) in escrow.locks.iter() {
            let fingerprint = LiquidityIntent::DexEscrow {
                escrow_id: *escrow_id,
                buy: buy.clone(),
                sell: sell.clone(),
                quantity: *qty,
                locked_at: *locked_at,
            }
            .fingerprint();
            let tie = Self::tie_break(entropy, &fingerprint, ordinal);
            ordinal = ordinal.saturating_add(1);
            let slot = self.compute_slot(*locked_at, BIAS_DEX, tie, fairness_range);
            sequenced.push(SequencedIntent::new(
                LiquidityIntent::DexEscrow {
                    escrow_id: *escrow_id,
                    buy: buy.clone(),
                    sell: sell.clone(),
                    quantity: *qty,
                    locked_at: *locked_at,
                },
                slot,
                tie,
            ));
        }

        for info in withdrawals {
            if info.challenged {
                continue;
            }
            if info.deadline > now_secs {
                continue;
            }
            let fingerprint = LiquidityIntent::BridgeWithdrawal {
                asset: info.asset.clone(),
                commitment: info.commitment,
                user: info.user.clone(),
                amount: info.amount,
                deadline: info.deadline,
            }
            .fingerprint();
            let tie = Self::tie_break(entropy, &fingerprint, ordinal);
            ordinal = ordinal.saturating_add(1);
            let slot = self.compute_slot(info.deadline, BIAS_BRIDGE, tie, fairness_range);
            sequenced.push(SequencedIntent::new(
                LiquidityIntent::BridgeWithdrawal {
                    asset: info.asset.clone(),
                    commitment: info.commitment,
                    user: info.user.clone(),
                    amount: info.amount,
                    deadline: info.deadline,
                },
                slot,
                tie,
            ));
        }

        let mut seen_pairs: HashSet<(String, String)> = HashSet::new();
        for ((from, to), line) in trust.lines_iter() {
            if line.balance <= 0 {
                continue;
            }
            let amount = match u64::try_from(line.balance) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if amount < self.config.min_trust_rebalance {
                continue;
            }
            let pair = (from.clone(), to.clone());
            if !seen_pairs.insert(pair.clone()) {
                continue;
            }
            if let Some((path, fallback)) = trust.find_best_path(from, to, amount) {
                let mut canonical_path = path;
                if canonical_path.len() - 1 > self.config.max_trust_hops {
                    if let Some(fallback_path) = fallback {
                        canonical_path = fallback_path;
                    }
                }
                if canonical_path.len() - 1 > self.config.max_trust_hops {
                    continue;
                }
                let fingerprint = LiquidityIntent::TrustRebalance {
                    path: canonical_path.clone(),
                    amount,
                }
                .fingerprint();
                let tie = Self::tie_break(entropy, &fingerprint, ordinal);
                ordinal = ordinal.saturating_add(1);
                let slot = self.compute_slot(now_secs, BIAS_TRUST, tie, fairness_range);
                sequenced.push(SequencedIntent::new(
                    LiquidityIntent::TrustRebalance {
                        path: canonical_path,
                        amount,
                    },
                    slot,
                    tie,
                ));
            }
        }

        sequenced.sort_by(|a, b| {
            if a.priority == b.priority {
                Ordering::Equal
            } else if a.priority < b.priority {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        });
        if sequenced.len() > self.config.batch_size {
            sequenced.truncate(self.config.batch_size);
        }
        LiquidityBatch {
            planned_at: now_secs,
            entropy,
            intents: sequenced,
        }
    }

    pub fn apply_batch(
        &self,
        batch: &LiquidityBatch,
        escrow: &mut EscrowState,
        mut store: Option<&mut DexStore>,
        trust: &mut TrustLedger,
        bridge: &mut Bridge,
    ) -> Result<LiquidityExecution, RouterError> {
        let mut execution = LiquidityExecution::default();
        for seq in &batch.intents {
            match &seq.intent {
                LiquidityIntent::DexEscrow {
                    escrow_id,
                    buy,
                    sell,
                    quantity,
                    ..
                } => {
                    let value = sell
                        .price
                        .checked_mul(*quantity)
                        .ok_or(RouterError::ArithmeticOverflow)?;
                    let (buy_clone, sell_clone, qty_clone, _locked) = escrow
                        .locks
                        .remove(escrow_id)
                        .ok_or(RouterError::EscrowMissing(*escrow_id))?;
                    let proof = escrow
                        .escrow
                        .release(*escrow_id, value)
                        .ok_or(RouterError::Dex("escrow release failed"))?;
                    if let Some(store) = store.as_deref_mut() {
                        store
                            .log_trade(&(buy_clone.clone(), sell_clone.clone(), qty_clone), &proof);
                        store.save_escrow_state(escrow);
                    }
                    if let Some((path, _)) =
                        trust.find_best_path(&buy.account, &sell.account, value)
                    {
                        if !trust.settle_path(&path, value) {
                            return Err(RouterError::TrustSettlementFailed(path));
                        }
                    } else {
                        return Err(RouterError::TrustPathUnavailable {
                            from: buy.account.clone(),
                            to: sell.account.clone(),
                            amount: value,
                        });
                    }
                    execution.released_escrows.push(*escrow_id);
                }
                LiquidityIntent::BridgeWithdrawal {
                    asset, commitment, ..
                } => {
                    bridge.finalize_withdrawal(asset, *commitment)?;
                    execution
                        .finalized_withdrawals
                        .push((asset.clone(), *commitment));
                }
                LiquidityIntent::TrustRebalance { path, amount } => {
                    if !trust.settle_path(path, *amount) {
                        return Err(RouterError::TrustSettlementFailed(path.clone()));
                    }
                    execution.trust_rebalances.push((path.clone(), *amount));
                }
            }
        }
        Ok(execution)
    }
}

impl Default for LiquidityRouter {
    fn default() -> Self {
        Self::new(RouterConfig::default())
    }
}

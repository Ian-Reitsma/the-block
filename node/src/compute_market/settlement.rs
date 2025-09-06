use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use ledger::utxo_account::AccountLedger;
#[cfg(feature = "telemetry")]
use crate::telemetry::SLASHING_BURN_CT_TOTAL;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettleMode {
    DryRun,
    Armed { activate_at: u64 },
    Real,
}

static MODE: Lazy<Mutex<SettleMode>> = Lazy::new(|| Mutex::new(SettleMode::DryRun));
static ACCOUNTS: Lazy<Mutex<AccountLedger>> = Lazy::new(|| Mutex::new(AccountLedger::new()));
// Separate ledger for industrial-token balances accrued via split payments.
static ACCOUNTS_IT: Lazy<Mutex<AccountLedger>> =
    Lazy::new(|| Mutex::new(AccountLedger::new()));

pub struct Settlement;

impl Settlement {
    pub fn init(_path: &str, mode: SettleMode) {
        *MODE.lock().unwrap_or_else(|e| e.into_inner()) = mode;
    }

    pub fn shutdown() {}

    pub fn penalize_sla(provider: &str, amount: u64) -> Result<(), ()> {
        let res = ACCOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .debit(provider, amount);
        if res.is_ok() {
            #[cfg(feature = "telemetry")]
            SLASHING_BURN_CT_TOTAL.inc_by(amount);
        }
        res.map_err(|_| ())
    }

    pub fn accrue(provider: &str, _event: &str, amount: u64) {
        ACCOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .deposit(provider, amount);
    }

    /// Credit a provider with a split CT/IT payout.
    pub fn accrue_split(provider: &str, ct: u64, it: u64) {
        ACCOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .deposit(provider, ct);
        ACCOUNTS_IT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .deposit(provider, it);
    }

    pub fn submit_anchor(_anchor: &[u8]) {}

    pub fn balance(provider: &str) -> u64 {
        ACCOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .balances
            .get(provider)
            .copied()
            .unwrap_or(0)
    }

    /// Return the CT/IT balances for a provider.
    pub fn balance_split(provider: &str) -> (u64, u64) {
        let ct = ACCOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .balances
            .get(provider)
            .copied()
            .unwrap_or(0);
        let it = ACCOUNTS_IT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .balances
            .get(provider)
            .copied()
            .unwrap_or(0);
        (ct, it)
    }

    pub fn spend(provider: &str, _event: &str, amount: u64) -> Result<(), ()> {
        ACCOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .debit(provider, amount)
            .map_err(|_| ())
    }

    /// Refund a buyer's escrowed CT/IT amounts.
    pub fn refund_split(buyer: &str, ct: u64, it: u64) {
        ACCOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .deposit(buyer, ct);
        ACCOUNTS_IT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .deposit(buyer, it);
    }

    pub fn mode() -> SettleMode {
        *MODE.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn arm(_delay: u64, _current_height: u64) {}

    pub fn cancel_arm() {}

    pub fn back_to_dry_run(_reason: &str) {}

    pub fn audit() -> Vec<()> {
        Vec::new()
    }

    pub fn recent_roots(_n: usize) -> Vec<[u8; 32]> {
        Vec::new()
    }
}

pub fn submit_anchor(_anchor: &[u8]) {}

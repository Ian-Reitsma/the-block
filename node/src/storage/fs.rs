use crate::simple_db::{names, SimpleDb};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    RENT_ESCROW_BURNED_CT_TOTAL, RENT_ESCROW_LOCKED_CT_TOTAL, RENT_ESCROW_REFUNDED_CT_TOTAL,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Escrow {
    depositor: String,
    amount: u64,
    expiry: u64,
}

pub struct RentEscrow {
    db: SimpleDb,
}

impl RentEscrow {
    pub fn open(path: &str) -> Self {
        Self {
            db: SimpleDb::open_named(names::STORAGE_FS, path),
        }
    }
    pub fn lock(&mut self, id: &str, depositor: &str, amount: u64, expiry: u64) {
        let key = format!("escrow/{id}");
        let e = Escrow {
            depositor: depositor.to_string(),
            amount,
            expiry,
        };
        if let Ok(bytes) = bincode::serialize(&e) {
            let _ = self.db.try_insert(&key, bytes);
            #[cfg(feature = "telemetry")]
            RENT_ESCROW_LOCKED_CT_TOTAL.add(amount as i64);
        }
    }
    pub fn release(&mut self, id: &str) -> Option<(String, u64, u64)> {
        let key = format!("escrow/{id}");
        if let Some(bytes) = self.db.get(&key) {
            if let Ok(e) = bincode::deserialize::<Escrow>(&bytes) {
                #[cfg(feature = "telemetry")]
                RENT_ESCROW_LOCKED_CT_TOTAL.sub(e.amount as i64);
                let _ = self.db.remove(&key);
                let refund = e.amount * 9 / 10;
                let burn = e.amount - refund;
                #[cfg(feature = "telemetry")]
                {
                    RENT_ESCROW_REFUNDED_CT_TOTAL.inc_by(refund as u64);
                    RENT_ESCROW_BURNED_CT_TOTAL.inc_by(burn as u64);
                }
                return Some((e.depositor, refund, burn));
            }
        }
        None
    }
    pub fn balance(&self, id: &str) -> u64 {
        let key = format!("escrow/{id}");
        if let Some(bytes) = self.db.get(&key) {
            if let Ok(e) = bincode::deserialize::<Escrow>(&bytes) {
                return e.amount;
            }
        }
        0
    }
    pub fn balance_account(&self, account: &str) -> u64 {
        let mut sum = 0;
        for key in self.db.keys_with_prefix("escrow/") {
            if let Some(bytes) = self.db.get(&key) {
                if let Ok(e) = bincode::deserialize::<Escrow>(&bytes) {
                    if e.depositor == account {
                        sum += e.amount;
                    }
                }
            }
        }
        sum
    }
    pub fn purge_expired(&mut self, now: u64) -> Vec<(String, u64, u64)> {
        let mut out = Vec::new();
        let keys = self.db.keys_with_prefix("escrow/");
        for key in keys {
            if let Some(bytes) = self.db.get(&key) {
                if let Ok(e) = bincode::deserialize::<Escrow>(&bytes) {
                    if e.expiry > 0 && e.expiry <= now {
                        #[cfg(feature = "telemetry")]
                        RENT_ESCROW_LOCKED_CT_TOTAL.sub(e.amount as i64);
                        let _ = self.db.remove(&key);
                        let refund = e.amount * 9 / 10;
                        let burn = e.amount - refund;
                        #[cfg(feature = "telemetry")]
                        {
                            RENT_ESCROW_REFUNDED_CT_TOTAL.inc_by(refund as u64);
                            RENT_ESCROW_BURNED_CT_TOTAL.inc_by(burn as u64);
                        }
                        out.push((e.depositor.clone(), refund, burn));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::RentEscrow;
    use tempfile::tempdir;

    #[test]
    fn release_refunds_and_burns() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rent.db");
        let mut escrow = RentEscrow::open(path.to_str().unwrap());
        escrow.lock("blob1", "alice", 1000, 0);
        let res = escrow.release("blob1").expect("entry");
        assert_eq!(res.0, "alice");
        assert_eq!(res.1, 900);
        assert_eq!(res.2, 100);
    }

    #[test]
    fn purge_expired_returns_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rent.db");
        let mut escrow = RentEscrow::open(path.to_str().unwrap());
        escrow.lock("blob", "bob", 1000, 1);
        let out = escrow.purge_expired(1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "bob");
        assert_eq!(out[0].1, 900);
        assert_eq!(out[0].2, 100);
    }
}

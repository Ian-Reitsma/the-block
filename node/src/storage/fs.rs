use crate::simple_db::{names, SimpleDb};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    RENT_ESCROW_BURNED_CT_TOTAL, RENT_ESCROW_LOCKED_CT_TOTAL, RENT_ESCROW_REFUNDED_CT_TOTAL,
};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted};
use foundation_serialization::binary_cursor::{Reader, Writer};

#[derive(Clone, Debug, PartialEq, Eq)]
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
        Self::open_with_factory(path, &SimpleDb::open_named)
    }

    pub fn open_with_factory<F>(path: &str, factory: &F) -> Self
    where
        F: Fn(&str, &str) -> SimpleDb,
    {
        Self {
            db: factory(names::STORAGE_FS, path),
        }
    }

    pub fn with_db(db: SimpleDb) -> Self {
        Self { db }
    }

    pub fn engine_label(&self) -> &'static str {
        self.db.backend_name()
    }
    pub fn lock(&mut self, id: &str, depositor: &str, amount: u64, expiry: u64) {
        let key = format!("escrow/{id}");
        let e = Escrow {
            depositor: depositor.to_string(),
            amount,
            expiry,
        };
        let bytes = encode_escrow(&e);
        let _ = self.db.try_insert(&key, bytes);
        #[cfg(feature = "telemetry")]
        RENT_ESCROW_LOCKED_CT_TOTAL.add(amount as i64);
    }
    pub fn release(&mut self, id: &str) -> Option<(String, u64, u64)> {
        let key = format!("escrow/{id}");
        if let Some(bytes) = self.db.get(&key) {
            if let Ok(e) = decode_escrow(&bytes) {
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
            if let Ok(e) = decode_escrow(&bytes) {
                return e.amount;
            }
        }
        0
    }
    pub fn balance_account(&self, account: &str) -> u64 {
        let mut sum = 0;
        for key in self.db.keys_with_prefix("escrow/") {
            if let Some(bytes) = self.db.get(&key) {
                if let Ok(e) = decode_escrow(&bytes) {
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
                if let Ok(e) = decode_escrow(&bytes) {
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

fn encode_escrow(record: &Escrow) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|s| {
        s.field_string("depositor", &record.depositor);
        s.field_u64("amount", record.amount);
        s.field_u64("expiry", record.expiry);
    });
    writer.finish()
}

fn decode_escrow(bytes: &[u8]) -> binary_struct::Result<Escrow> {
    let mut reader = Reader::new(bytes);
    let mut depositor = None;
    let mut amount = None;
    let mut expiry = None;
    decode_struct(&mut reader, Some(3), |key, reader| match key {
        "depositor" => {
            let value = reader.read_string()?;
            assign_once(&mut depositor, value, "depositor")
        }
        "amount" => {
            let value = reader.read_u64()?;
            assign_once(&mut amount, value, "amount")
        }
        "expiry" => {
            let value = reader.read_u64()?;
            assign_once(&mut expiry, value, "expiry")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    ensure_exhausted(&reader)?;
    Ok(Escrow {
        depositor: depositor.ok_or(binary_struct::DecodeError::MissingField("depositor"))?,
        amount: amount.ok_or(binary_struct::DecodeError::MissingField("amount"))?,
        expiry: expiry.ok_or(binary_struct::DecodeError::MissingField("expiry"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::RentEscrow;
    use super::{decode_escrow, encode_escrow, Escrow};
    use foundation_serialization::binary_cursor::Writer;
    use foundation_serialization::{Deserialize, Serialize};
    use sys::tempfile::tempdir;

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

    #[derive(Serialize, Deserialize)]
    #[serde(crate = "foundation_serialization::serde")]
    struct LegacyEscrow {
        depositor: String,
        amount: u64,
        expiry: u64,
    }

    #[test]
    fn escrow_binary_matches_legacy() {
        let record = Escrow {
            depositor: "alice".to_string(),
            amount: 4242,
            expiry: 99,
        };
        let encoded = encode_escrow(&record);
        let legacy_record = LegacyEscrow {
            depositor: record.depositor.clone(),
            amount: record.amount,
            expiry: record.expiry,
        };
        let legacy = encode_legacy(&legacy_record);
        assert_eq!(encoded, legacy);

        let decoded = decode_escrow(&encoded).expect("decode");
        assert_eq!(decoded, record);
    }

    fn encode_legacy(record: &LegacyEscrow) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.write_struct(|s| {
            s.field_string("depositor", &record.depositor);
            s.field_u64("amount", record.amount);
            s.field_u64("expiry", record.expiry);
        });
        writer.finish()
    }
}

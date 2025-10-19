#![forbid(unsafe_code)]

use crate::vm::contracts::htlc::Htlc;
use concurrency::Lazy;
use std::collections::BTreeMap;
use std::sync::Mutex;

use foundation_serialization::Serialize;

static STORE: Lazy<Mutex<BTreeMap<u64, Htlc>>> = Lazy::new(|| Mutex::new(BTreeMap::new()));

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde", untagged)]
pub enum HtlcStatusResponse {
    Status {
        hash: String,
        timeout: u64,
        redeemed: bool,
    },
    Error {
        error: &'static str,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde", untagged)]
pub enum HtlcRefundResponse {
    Status { status: &'static str },
    Error { error: &'static str },
}

pub fn insert(id: u64, h: Htlc) {
    let mut store = STORE.lock().unwrap();
    store.insert(id, h);
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::HTLC_CREATED_TOTAL.inc();
    }
}

pub fn status(id: u64) -> HtlcStatusResponse {
    let store = STORE.lock().unwrap();
    if let Some(h) = store.get(&id) {
        HtlcStatusResponse::Status {
            hash: crypto_suite::hex::encode(&h.hash),
            timeout: h.timeout,
            redeemed: h.redeemed,
        }
    } else {
        HtlcStatusResponse::Error { error: "not_found" }
    }
}

pub fn refund(id: u64, now: u64) -> HtlcRefundResponse {
    let mut store = STORE.lock().unwrap();
    if let Some(h) = store.get_mut(&id) {
        if h.refund(now) {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::HTLC_REFUNDED_TOTAL.inc();
            }
            HtlcRefundResponse::Status { status: "refunded" }
        } else {
            HtlcRefundResponse::Error {
                error: "not_refundable",
            }
        }
    } else {
        HtlcRefundResponse::Error { error: "not_found" }
    }
}

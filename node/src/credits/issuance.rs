use std::collections::HashMap;
use std::sync::RwLock;

use once_cell::sync::Lazy;

use credits::Source;

use crate::compute_market::settlement::Settlement;
#[cfg(feature = "telemetry")]
use crate::telemetry::{CREDIT_ISSUED_TOTAL, CREDIT_ISSUE_REJECTED_TOTAL};

#[derive(Clone)]
pub struct IssuanceParams {
    pub weights_ppm: HashMap<Source, u64>,
    pub cap_per_identity: u64,
    pub cap_per_region: u64,
    pub expiry_days: HashMap<Source, u64>,
}

impl Default for IssuanceParams {
    fn default() -> Self {
        let mut weights_ppm = HashMap::new();
        weights_ppm.insert(Source::Uptime, 1_000_000);
        weights_ppm.insert(Source::LocalNetAssist, 1_000_000);
        weights_ppm.insert(Source::ProvenStorage, 1_000_000);
        weights_ppm.insert(Source::Civic, 1_000_000);
        let mut expiry_days = HashMap::new();
        expiry_days.insert(Source::Uptime, u64::MAX);
        expiry_days.insert(Source::LocalNetAssist, u64::MAX);
        expiry_days.insert(Source::ProvenStorage, u64::MAX);
        expiry_days.insert(Source::Civic, u64::MAX);
        Self {
            weights_ppm,
            cap_per_identity: u64::MAX,
            cap_per_region: u64::MAX,
            expiry_days,
        }
    }
}

#[derive(Default)]
struct IssuanceState {
    params: IssuanceParams,
    identity_totals: HashMap<String, u64>,
    region_totals: HashMap<String, u64>,
}

static STATE: Lazy<RwLock<IssuanceState>> = Lazy::new(|| RwLock::new(IssuanceState::default()));

#[cfg(feature = "telemetry")]
fn src_label(s: Source) -> &'static str {
    match s {
        Source::Uptime => "uptime",
        Source::LocalNetAssist => "localnet",
        Source::ProvenStorage => "storage",
        Source::Civic => "civic",
    }
}

pub fn set_params(p: IssuanceParams) {
    let mut st = STATE.write().unwrap();
    st.params = p;
}

pub fn issue(provider: &str, region: &str, source: Source, event: &str, base_amount: u64) {
    let mut st = STATE.write().unwrap();
    let params = st.params.clone();
    let weight_ppm = *params.weights_ppm.get(&source).unwrap_or(&1_000_000);
    let amount = ((base_amount as u128 * weight_ppm as u128) / 1_000_000u128) as u64;
    let id_total = *st.identity_totals.get(provider).unwrap_or(&0);
    if id_total + amount > params.cap_per_identity {
        #[cfg(feature = "telemetry")]
        {
            CREDIT_ISSUE_REJECTED_TOTAL
                .with_label_values(&["identity_cap"])
                .inc();
        }
        return;
    }
    let reg_total = *st.region_totals.get(region).unwrap_or(&0);
    if reg_total + amount > params.cap_per_region {
        #[cfg(feature = "telemetry")]
        {
            CREDIT_ISSUE_REJECTED_TOTAL
                .with_label_values(&["region_cap"])
                .inc();
        }
        return;
    }
    let expiry_days = *params.expiry_days.get(&source).unwrap_or(&u64::MAX);
    Settlement::accrue(provider, event, source, amount, expiry_days);
    *st.identity_totals.entry(provider.to_owned()).or_default() += amount;
    *st.region_totals.entry(region.to_owned()).or_default() += amount;
    #[cfg(feature = "telemetry")]
    {
        CREDIT_ISSUED_TOTAL
            .with_label_values(&[src_label(source)])
            .inc_by(amount);
    }
}

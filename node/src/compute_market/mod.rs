#[cfg(feature = "telemetry")]
use crate::telemetry;
use crate::transaction::FeeLane;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

pub mod admission;
pub mod cbm;
pub mod courier;
pub mod courier_store;
pub mod errors;
pub mod matcher;
pub mod price_board;
pub mod receipt;
pub mod scheduler;
pub mod settlement;
pub mod workload;
pub mod workloads;

pub use errors::MarketError;

/// Minimum bond required on each side of a compute offer.
pub const MIN_BOND: u64 = 1;

/// A stake-backed offer for compute capacity.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Offer {
    pub job_id: String,
    pub provider: String,
    pub provider_bond: u64,
    pub consumer_bond: u64,
    /// Total compute units the provider is willing to execute.
    pub units: u64,
    /// Price quoted per compute unit.
    pub price_per_unit: u64,
    /// Percentage of `price` paid in consumer tokens. `0` routes the entire
    /// amount to industrial tokens, `100` routes it all to consumer tokens.
    pub fee_pct_ct: u8,
    /// Hardware capability advertised by the provider.
    #[serde(default)]
    pub capability: scheduler::Capability,
    /// Initial reputation score for the provider.
    #[serde(default)]
    pub reputation: i64,
    /// Reputation-based price multiplier.
    #[serde(default = "default_multiplier")]
    pub reputation_multiplier: f64,
}

impl Offer {
    /// Validate that both sides posted at least `MIN_BOND`.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.provider_bond < MIN_BOND {
            return Err("provider bond too low");
        }
        if self.consumer_bond < MIN_BOND {
            return Err("consumer bond too low");
        }
        if self.units == 0 {
            return Err("no units offered");
        }
        if self.fee_pct_ct > 100 {
            return Err("invalid fee_pct_ct");
        }
        if !scheduler::validate_multiplier(self.reputation_multiplier) {
            return Err("invalid reputation multiplier");
        }
        Ok(())
    }
}

fn default_multiplier() -> f64 {
    1.0
}

/// A single slice of a job with a reference hash and output hash.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SliceProof {
    pub reference: [u8; 32],
    pub output: [u8; 32],
    pub payout: u64,
}

impl SliceProof {
    /// Verify that the output matches the reference hash.
    pub fn verify(&self) -> bool {
        self.reference == self.output
    }
}

/// Compute price bands (p25, median, p75) for dashboard display.
pub fn price_bands(prices: &[u64]) -> Option<(u64, u64, u64)> {
    if prices.is_empty() {
        return None;
    }
    let mut p = prices.to_vec();
    p.sort_unstable();
    let median = p[p.len() / 2];
    let p25 = p[(p.len() as f64 * 0.25).floor() as usize];
    let p75 = p[(p.len() as f64 * 0.75).floor() as usize];
    Some((p25, median, p75))
}

/// Adjust the median price by a backlog factor.
pub fn adjust_price(median: u64, backlog_factor: f64) -> u64 {
    (median as f64 * backlog_factor).round() as u64
}

/// A workload describes real compute to run for a job slice.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Workload {
    Transcode(Vec<u8>),
    Inference(Vec<u8>),
    GpuHash(Vec<u8>),
}

impl Workload {
    /// Estimate normalized compute units for this workload using the generic
    /// `compute_units` helper.
    pub fn units(&self) -> u64 {
        match self {
            Workload::Transcode(data) | Workload::Inference(data) | Workload::GpuHash(data) => {
                workload::compute_units(data)
            }
        }
    }
}

/// Execute workloads and produce proof hashes with per-slice caching.
pub struct WorkloadRunner {
    cache: std::sync::Arc<std::sync::Mutex<HashMap<usize, [u8; 32]>>>,
}

impl WorkloadRunner {
    pub fn new() -> Self {
        Self {
            cache: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Run the workload for a given slice ID asynchronously. Results are cached so
    /// repeated executions avoid recomputation.
    pub async fn run(&self, slice_id: usize, w: Workload) -> [u8; 32] {
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&slice_id)
        {
            return *cached;
        }
        let res = tokio::task::spawn_blocking(move || match w {
            Workload::Transcode(data) => workloads::transcode::run(&data),
            Workload::Inference(data) => workloads::inference::run(&data),
            Workload::GpuHash(data) => workloads::gpu::run(&data),
        })
        .await
        .unwrap_or_else(|e| panic!("workload failed: {e}"));
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(slice_id, res);
        res
    }
}

/// A job submitted by a consumer with per-slice reference hashes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub job_id: String,
    pub buyer: String,
    pub slices: Vec<[u8; 32]>,
    /// Maximum price per compute unit the buyer is willing to pay.
    pub price_per_unit: u64,
    pub consumer_bond: u64,
    pub workloads: Vec<Workload>,
    /// Required hardware capability for the job.
    #[serde(default)]
    pub capability: scheduler::Capability,
    /// Unix timestamp by which the provider must deliver.
    pub deadline: u64,
}

/// Internal state for a matched job.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct JobState {
    job: Job,
    provider: String,
    provider_bond: u64,
    price_per_unit: u64,
    paid_slices: usize,
    completed: bool,
}

/// In-memory market tracking offers and active jobs.
#[derive(Default)]
pub struct Market {
    offers: HashMap<String, Offer>,
    jobs: HashMap<String, JobState>,
}

impl Market {
    /// Create an empty market.
    pub fn new() -> Self {
        admission::record_available_shards(100);
        Self::default()
    }

    /// Post an offer from a provider.
    pub fn post_offer(&mut self, offer: Offer) -> Result<(), &'static str> {
        let mut offer = offer;
        offer.validate()?;
        if offer.price_per_unit == 0 {
            if let Some(p) = price_board::backlog_adjusted_bid(FeeLane::Industrial, self.jobs.len())
            {
                offer.price_per_unit = p;
            }
        }
        if self.jobs.contains_key(&offer.job_id) {
            if scheduler::try_preempt(&offer.job_id, &offer.provider, offer.reputation) {
                if let Some(state) = self.jobs.get_mut(&offer.job_id) {
                    state.provider = offer.provider.clone();
                }
                return Ok(());
            } else {
                return Err("preemption rejected");
            }
        }
        if self.offers.contains_key(&offer.job_id) {
            return Err("offer already exists");
        }
        scheduler::register_offer(
            &offer.provider,
            offer.capability.clone(),
            offer.reputation,
            offer.price_per_unit,
            offer.reputation_multiplier,
        );
        self.offers.insert(offer.job_id.clone(), offer);
        Ok(())
    }

    /// Submit a job from the consumer side, matching an existing offer.
    pub fn submit_job(&mut self, job: Job) -> Result<(), MarketError> {
        if job.consumer_bond < MIN_BOND {
            return Err(MarketError::InvalidWorkload);
        }
        if job.workloads.len() != job.slices.len() {
            return Err(MarketError::InvalidWorkload);
        }
        let offer = self
            .offers
            .get(&job.job_id)
            .cloned()
            .ok_or(MarketError::JobNotFound)?;
        if !offer.capability.matches(&job.capability) {
            return Err(MarketError::Capability);
        }
        let demand: u64 = job.workloads.iter().map(|w| w.units()).sum();
        if let Err(reason) = admission::check_and_record(&job.buyer, &offer.provider, demand) {
            #[cfg(feature = "telemetry")]
            {
                use admission::RejectReason::*;
                match reason {
                    Capacity => {
                        telemetry::INDUSTRIAL_REJECTED_TOTAL
                            .with_label_values(&["capacity"])
                            .inc();
                        telemetry::INDUSTRIAL_DEFERRED_TOTAL.inc();
                        return Err(MarketError::Capacity);
                    }
                    FairShare => {
                        telemetry::INDUSTRIAL_REJECTED_TOTAL
                            .with_label_values(&["fair_share"])
                            .inc();
                        return Err(MarketError::FairShare);
                    }
                    BurstExhausted => {
                        telemetry::INDUSTRIAL_REJECTED_TOTAL
                            .with_label_values(&["burst_exhausted"])
                            .inc();
                        return Err(MarketError::BurstExhausted);
                    }
                }
            }
            #[cfg(not(feature = "telemetry"))]
            {
                use admission::RejectReason::*;
                return Err(match reason {
                    Capacity => MarketError::Capacity,
                    FairShare => MarketError::FairShare,
                    BurstExhausted => MarketError::BurstExhausted,
                });
            }
        }
        let offer = self
            .offers
            .remove(&job.job_id)
            .ok_or(MarketError::JobNotFound)?;
        price_board::record_price(
            FeeLane::Industrial,
            offer.price_per_unit,
            offer.reputation_multiplier,
        );
        let effective = (offer.price_per_unit as f64 * offer.reputation_multiplier).round() as u64;
        #[cfg(feature = "telemetry")]
        telemetry::SCHEDULER_EFFECTIVE_PRICE
            .with_label_values(&[&offer.provider])
            .set(effective as i64);
        let state = JobState {
            job,
            provider: offer.provider.clone(),
            provider_bond: offer.provider_bond,
            price_per_unit: offer.price_per_unit,
            paid_slices: 0,
            completed: false,
        };
        scheduler::start_job(&offer.job_id, &offer.provider, state.job.capability.clone());
        self.jobs.insert(state.job.job_id.clone(), state);
        #[cfg(feature = "telemetry")]
        telemetry::INDUSTRIAL_ADMITTED_TOTAL.inc();
        Ok(())
    }

    /// Cancel an unmatched offer and return it.
    pub fn cancel_offer(&mut self, job_id: &str) -> Option<Offer> {
        self.offers.remove(job_id)
    }

    /// Cancel an in-flight job, returning both bonds.
    pub fn cancel_job(&mut self, job_id: &str) -> Option<(u64, u64)> {
        let state = self.jobs.remove(job_id)?;
        scheduler::end_job(job_id);
        Some((state.provider_bond, state.job.consumer_bond))
    }

    /// Verify a slice proof and record the payout.
    pub fn submit_slice(&mut self, job_id: &str, proof: SliceProof) -> Result<u64, &'static str> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let state = self.jobs.get_mut(job_id).ok_or("unknown job")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time: {e}"))
            .as_secs();
        if now > state.job.deadline {
            scheduler::record_failure(&state.provider);
            let _ = settlement::Settlement::penalize_sla(&state.provider, state.provider_bond);
            self.jobs.remove(job_id);
            scheduler::end_job(job_id);
            return Err("deadline exceeded");
        }
        if state.completed {
            return Err("job already completed");
        }
        let expected_ref = state
            .job
            .slices
            .get(state.paid_slices)
            .ok_or("no such slice")?;
        if &proof.reference != expected_ref {
            scheduler::record_failure(&state.provider);
            return Err("reference mismatch");
        }
        if !proof.verify() {
            scheduler::record_failure(&state.provider);
            return Err("invalid proof");
        }
        let slice_units = state.job.workloads[state.paid_slices].units();
        if proof.payout != slice_units * state.price_per_unit {
            scheduler::record_failure(&state.provider);
            return Err("payout mismatch");
        }
        #[cfg(feature = "telemetry")]
        telemetry::INDUSTRIAL_UNITS_TOTAL.inc_by(slice_units as u64);
        state.paid_slices += 1;
        if state.paid_slices == state.job.slices.len() {
            state.completed = true;
        }
        Ok(proof.payout)
    }

    /// Finalize a job and release bonds if complete.
    pub fn finalize_job(&mut self, job_id: &str) -> Option<(u64, u64)> {
        let state = self.jobs.get(job_id)?;
        if !state.completed {
            scheduler::record_failure(&state.provider);
            return None;
        }
        scheduler::record_success(&state.provider);
        let provider_bond = state.provider_bond;
        let consumer_bond = state.job.consumer_bond;
        self.jobs.remove(job_id);
        scheduler::end_job(job_id);
        Some((provider_bond, consumer_bond))
    }

    /// Execute a job by submitting slice outputs and returning total payout.
    pub async fn execute_job(&mut self, job_id: &str) -> Result<u64, &'static str> {
        let (slices, workloads, price_per_unit) = {
            let state = self.jobs.get(job_id).ok_or("unknown job")?;
            (
                state.job.slices.clone(),
                state.job.workloads.clone(),
                state.price_per_unit,
            )
        };
        let runner = WorkloadRunner::new();
        let mut handles = Vec::new();
        for (i, w) in workloads.iter().cloned().enumerate() {
            handles.push(runner.run(i, w));
        }
        let results = futures::future::join_all(handles).await;
        let mut total = 0;
        for (expected, (output, w)) in slices
            .into_iter()
            .zip(results.into_iter().zip(workloads.into_iter()))
        {
            let units = w.units();
            let proof = SliceProof {
                reference: expected,
                output,
                payout: units * price_per_unit,
            };
            total += self.submit_slice(job_id, proof)?;
        }
        Ok(total)
    }

    /// Compute a backlog factor based on pending slices vs. available capacity.
    pub fn backlog_factor(&self) -> f64 {
        let pending: u64 = self
            .jobs
            .values()
            .map(|s| {
                s.job
                    .workloads
                    .iter()
                    .skip(s.paid_slices)
                    .map(|w| w.units())
                    .sum::<u64>()
            })
            .sum();
        let capacity: u64 = self.offers.values().map(|o| o.units).sum();
        crate::compute_market::price_board::report_backlog(pending, capacity);
        if capacity == 0 {
            1.0 + pending as f64
        } else {
            1.0 + pending as f64 / capacity as f64
        }
    }
}

/// Track recent prices and compute bands.
pub struct PriceBoard {
    prices: VecDeque<u64>,
    max: usize,
}

impl Default for PriceBoard {
    fn default() -> Self {
        Self::new(100)
    }
}

impl PriceBoard {
    /// Create a board retaining up to `max` prices.
    pub fn new(max: usize) -> Self {
        Self {
            prices: VecDeque::new(),
            max,
        }
    }

    /// Record a new price observation, trimming to `max` entries.
    pub fn record(&mut self, price: u64) {
        if self.prices.len() == self.max {
            self.prices.pop_front();
        }
        self.prices.push_back(price);
    }

    /// Return p25/median/p75 bands from observed prices.
    pub fn bands(&self) -> Option<(u64, u64, u64)> {
        let v: Vec<u64> = self.prices.iter().copied().collect();
        price_bands(&v)
    }

    /// Return a backlog-adjusted median price.
    pub fn adjusted_median(&self, backlog_factor: f64) -> Option<u64> {
        self.bands()
            .map(|(_, median, _)| adjust_price(median, backlog_factor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blake3::Hasher;

    #[test]
    fn offer_requires_bonds() {
        let offer = Offer {
            job_id: "job".into(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 10,
            price_per_unit: 5,
            fee_pct_ct: 100,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        assert!(offer.validate().is_ok());
    }

    #[test]
    fn slice_verification() {
        let data = b"hello";
        let mut h = Hasher::new();
        h.update(data);
        let hash = *h.finalize().as_bytes();
        let proof = SliceProof {
            reference: hash,
            output: hash,
            payout: 1,
        };
        assert!(proof.verify());
    }

    #[test]
    fn price_band_calc() {
        let bands = price_bands(&[1, 2, 3, 4]).unwrap_or_else(|| panic!("price bands"));
        assert_eq!(bands, (2, 3, 4));
    }

    #[test]
    fn courier_store_forward() {
        use crate::compute_market::courier::CourierStore;
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("create temp dir: {e}"));
        let store = CourierStore::open(
            dir.path()
                .to_str()
                .unwrap_or_else(|| panic!("temp dir path")),
        );
        let receipt = store.send(b"bundle", "alice");
        assert!(!receipt.acknowledged);
        let forwarded = store
            .flush(|r| r.sender == "alice")
            .unwrap_or_else(|e| panic!("flush receipts: {e}"));
        assert_eq!(forwarded, 1);
        let stored = store
            .get(receipt.id)
            .unwrap_or_else(|| panic!("missing receipt"));
        assert!(stored.acknowledged);
    }

    #[test]
    fn job_lifecycle_and_finalize() {
        let mut market = Market::new();
        let job_id = "job1".to_string();
        let offer = Offer {
            job_id: job_id.clone(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 1,
            price_per_unit: 5,
            fee_pct_ct: 100,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        market
            .post_offer(offer)
            .unwrap_or_else(|e| panic!("post offer: {e}"));
        let mut h = Hasher::new();
        h.update(b"slice");
        let hash = *h.finalize().as_bytes();
        let job = Job {
            job_id: job_id.clone(),
            buyer: "buyer".into(),
            slices: vec![hash],
            price_per_unit: 5,
            consumer_bond: 1,
            workloads: vec![Workload::Transcode(b"slice".to_vec())],
            capability: scheduler::Capability::default(),
            deadline: u64::MAX,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        let proof = SliceProof {
            reference: hash,
            output: hash,
            payout: 5,
        };
        assert_eq!(
            market
                .submit_slice(&job_id, proof)
                .unwrap_or_else(|e| panic!("submit slice: {e}")),
            5
        );
        let bonds = market
            .finalize_job(&job_id)
            .unwrap_or_else(|| panic!("finalize job"));
        assert_eq!(bonds, (1, 1));
    }

    #[test]
    fn backlog_adjusts_price() {
        let mut market = Market::new();
        let mut h = Hasher::new();
        h.update(b"a");
        let hash = *h.finalize().as_bytes();
        let offer = Offer {
            job_id: "j1".into(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 1,
            price_per_unit: 5,
            fee_pct_ct: 100,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        market
            .post_offer(offer)
            .unwrap_or_else(|e| panic!("post offer: {e}"));
        let job = Job {
            job_id: "j1".into(),
            buyer: "buyer".into(),
            slices: vec![hash, hash],
            price_per_unit: 5,
            consumer_bond: 1,
            workloads: vec![
                Workload::Transcode(b"a".to_vec()),
                Workload::Transcode(b"a".to_vec()),
            ],
            capability: scheduler::Capability::default(),
            deadline: u64::MAX,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        assert!(market.backlog_factor() > 1.0);
        let mut board = PriceBoard::new(10);
        board.record(5);
        let adj = board
            .adjusted_median(market.backlog_factor())
            .unwrap_or_else(|| panic!("adjusted median"));
        assert!(adj >= 5);
    }

    #[test]
    fn cancel_paths() {
        let mut market = Market::new();
        let offer = Offer {
            job_id: "j2".into(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 1,
            price_per_unit: 5,
            fee_pct_ct: 100,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        market
            .post_offer(offer.clone())
            .unwrap_or_else(|e| panic!("post offer: {e}"));
        assert!(market.cancel_offer("j2").is_some());
        market
            .post_offer(offer)
            .unwrap_or_else(|e| panic!("post offer: {e}"));
        let mut h = Hasher::new();
        h.update(b"slice");
        let hash = *h.finalize().as_bytes();
        let job = Job {
            job_id: "j2".into(),
            buyer: "buyer".into(),
            slices: vec![hash],
            price_per_unit: 5,
            consumer_bond: 1,
            workloads: vec![Workload::Transcode(b"slice".to_vec())],
            capability: scheduler::Capability::default(),
            deadline: u64::MAX,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        let bonds = market
            .cancel_job("j2")
            .unwrap_or_else(|| panic!("cancel job"));
        assert_eq!(bonds, (1, 1));
    }

    #[test]
    fn execute_job_path() {
        let mut market = Market::new();
        let job_id = "exec".to_string();
        let offer = Offer {
            job_id: job_id.clone(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 1,
            price_per_unit: 2,
            fee_pct_ct: 100,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        market
            .post_offer(offer)
            .unwrap_or_else(|e| panic!("post offer: {e}"));
        let mut h = Hasher::new();
        h.update(b"a");
        let hash = *h.finalize().as_bytes();
        let job = Job {
            job_id: job_id.clone(),
            buyer: "buyer".into(),
            slices: vec![hash],
            price_per_unit: 2,
            consumer_bond: 1,
            workloads: vec![Workload::Transcode(b"a".to_vec())],
            capability: scheduler::Capability::default(),
            deadline: u64::MAX,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| panic!("runtime: {e}"));
        let total = rt
            .block_on(market.execute_job(&job_id))
            .unwrap_or_else(|e| panic!("execute job: {e}"));
        assert_eq!(total, 2);
        let bonds = market
            .finalize_job(&job_id)
            .unwrap_or_else(|| panic!("finalize job"));
        assert_eq!(bonds, (1, 1));
    }
}

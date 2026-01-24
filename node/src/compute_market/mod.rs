use self::snark::{SnarkBackend, SnarkError};
use crate::receipts::BlockTorchReceiptMetadata;
#[cfg(feature = "telemetry")]
use crate::telemetry;
use crate::transaction::FeeLane;
use concurrency::{mutex, MutexExt, MutexT};
use foundation_serialization::{Deserialize, Serialize};
use settlement::{SlaOutcome, SlaResolutionKind};
use std::collections::{HashMap, VecDeque};
use std::env;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sys::cpu;

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
mod tensor_profile;
pub mod workload;

#[cfg(doctest)]
#[doc = concat!("```rust\n", include_str!("../../examples/compute_market.rs"), "\n```")]
mod compute_market_example {}
pub mod snark;
pub mod workloads;

use workloads::blocktorch;
use workloads::inference::BlockTorchInference;
use workloads::{BlockTorchWorkloadMetadata, WorkloadRunOutput};

pub use errors::MarketError;
pub use scheduler::job_status;

static TOTAL_UNITS_PROCESSED: AtomicU64 = AtomicU64::new(0);

/// Return the total number of compute units settled since startup.
pub fn total_units_processed() -> u64 {
    TOTAL_UNITS_PROCESSED.load(Ordering::Relaxed)
}

/// Reset the processed-unit counter. Intended for tests.
pub fn reset_units_processed_for_test() {
    TOTAL_UNITS_PROCESSED.store(0, Ordering::Relaxed);
}

fn record_units_processed(units: u64) {
    TOTAL_UNITS_PROCESSED.fetch_add(units, Ordering::Relaxed);
    #[cfg(feature = "telemetry")]
    telemetry::INDUSTRIAL_UNITS_TOTAL.inc_by(units);
}

/// Supported specialised accelerators.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Accelerator {
    Fpga,
    Tpu,
}

/// Minimum bond required on each side of a compute offer.
pub const MIN_BOND: u64 = 1;

/// A stake-backed offer for compute capacity.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Offer {
    pub job_id: String,
    pub provider: String,
    pub provider_bond: u64,
    pub consumer_bond: u64,
    /// Total compute units the provider is willing to execute.
    pub units: u64,
    /// Price quoted per compute unit.
    pub price_per_unit: u64,
    /// Percentage of `price` routed to the consumer lane. `0` routes the entire
    /// amount to the industrial lane, `100` routes it all to the consumer lane.
    pub fee_pct: u8,
    /// Hardware capability advertised by the provider.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub capability: scheduler::Capability,
    /// Initial reputation score for the provider.
    #[serde(default = "foundation_serialization::defaults::default")]
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
        if self.fee_pct > 100 {
            return Err("invalid fee_pct");
        }
        if !scheduler::validate_multiplier(self.reputation_multiplier) {
            return Err("invalid reputation multiplier");
        }
        if let Err(e) = self.capability.validate() {
            return Err(e);
        }
        Ok(())
    }

    fn effective_reputation_multiplier(&self) -> f64 {
        let baseline = default_multiplier();
        if self.reputation_multiplier == 0.0 {
            baseline
        } else {
            self.reputation_multiplier
        }
    }
}

fn default_multiplier() -> f64 {
    1.0
}

/// Receipt for a workload slice including an optional SNARK proof.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ExecutionReceipt {
    pub reference: [u8; 32],
    pub output: [u8; 32],
    #[serde(default)]
    pub payout: u64,
    #[serde(
        default = "foundation_serialization::defaults::default",
        skip_serializing_if = "foundation_serialization::skip::option_is_none"
    )]
    pub proof: Option<snark::ProofBundle>,
}

impl ExecutionReceipt {
    /// Verify that the output matches the reference hash and any provided proof.
    pub fn verify_with_duration(&self, workload: &Workload) -> (bool, Option<std::time::Duration>) {
        if self.reference != self.output {
            return (false, None);
        }
        match (&self.proof, workload) {
            (Some(bundle), Workload::Snark(wasm)) => {
                let start = Instant::now();
                let result = snark::verify(bundle, wasm, &self.output).unwrap_or(false);
                (result, Some(start.elapsed()))
            }
            (None, Workload::Snark(_)) => (false, None),
            (Some(_), _) => (false, None),
            _ => (true, None),
        }
    }

    pub fn verify(&self, workload: &Workload) -> bool {
        self.verify_with_duration(workload).0
    }

    pub fn total(&self) -> u64 {
        self.payout
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
    Inference(BlockTorchInference),
    GpuHash(Vec<u8>),
    Snark(Vec<u8>),
}

impl Workload {
    /// Estimate normalized compute units for this workload using the generic
    /// `compute_units` helper.
    pub fn units(&self) -> u64 {
        match self {
            Workload::Transcode(data) | Workload::GpuHash(data) | Workload::Snark(data) => {
                workload::compute_units(data)
            }
            Workload::Inference(payload) => workload::compute_units(&payload.artifact)
                .saturating_add(workload::compute_units(&payload.input)),
        }
    }
}

/// Execute workloads and produce proof hashes with per-slice caching.
pub struct WorkloadRunner {
    cache: Arc<MutexT<HashMap<usize, WorkloadRunOutput>>>,
}

impl WorkloadRunner {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(mutex(HashMap::new())),
        }
    }

    /// Detect host hardware capability for scheduling.
    pub fn hardware_capability() -> scheduler::Capability {
        let cpu = cpu::logical_count() as u8;
        let gpu = std::env::var("TB_GPU_MODEL").ok();
        let frameworks = std::env::var("TB_FRAMEWORKS")
            .ok()
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        scheduler::Capability {
            cpu_cores: cpu,
            gpu,
            frameworks,
            ..Default::default()
        }
    }

    /// Run the workload for a given slice ID asynchronously. Results are cached so
    /// repeated executions avoid recomputation.
    pub async fn run(&self, slice_id: usize, w: Workload) -> WorkloadRunOutput {
        if let Some(cached) = self.cache.guard().get(&slice_id) {
            return cached.clone();
        }
        let res = runtime::spawn_blocking(move || match w {
            Workload::Transcode(data) => WorkloadRunOutput::plain(workloads::transcode::run(&data)),
            Workload::Inference(data) => workloads::inference::run(&data),
            Workload::GpuHash(data) => workloads::gpu::run(&data),
            Workload::Snark(data) => WorkloadRunOutput::plain(workloads::snark::run(&data)),
        })
        .await
        .unwrap_or_else(|e| panic!("workload failed: {e}"));
        self.cache.guard().insert(slice_id, res.clone());
        res
    }
}

fn record_blocktorch_metadata(meta: &BlockTorchWorkloadMetadata) {
    let (benchmark_commit, tensor_profile_epoch) = resolved_blocktorch_strings(meta);
    #[cfg(feature = "telemetry")]
    {
        telemetry::receipts::set_blocktorch_kernel_digest(meta.kernel_digest);
        telemetry::receipts::set_blocktorch_benchmark_commit(benchmark_commit.as_deref());
        telemetry::receipts::set_blocktorch_tensor_profile_epoch(tensor_profile_epoch.as_deref());
        telemetry::receipts::set_blocktorch_descriptor_digest(meta.descriptor_digest);
        telemetry::receipts::set_blocktorch_output_digest(meta.output_digest);
    }
}

fn fallback_blocktorch_string(value: &Option<String>, env_key: &str) -> Option<String> {
    value.clone().or_else(|| env::var(env_key).ok())
}

fn resolved_blocktorch_strings(
    meta: &BlockTorchWorkloadMetadata,
) -> (Option<String>, Option<String>) {
    (
        fallback_blocktorch_string(&meta.benchmark_commit, "TB_BLOCKTORCH_BENCHMARK_COMMIT"),
        fallback_blocktorch_string(
            &meta.tensor_profile_epoch,
            "TB_BLOCKTORCH_TENSOR_PROFILE_EPOCH",
        ),
    )
}

fn blocktorch_receipt_metadata(
    meta: &BlockTorchWorkloadMetadata,
    proof_latency_ms: u64,
) -> BlockTorchReceiptMetadata {
    let (benchmark_commit, tensor_profile_epoch) = resolved_blocktorch_strings(meta);
    BlockTorchReceiptMetadata {
        kernel_variant_digest: meta.kernel_digest,
        descriptor_digest: meta.descriptor_digest,
        output_digest: meta.output_digest,
        benchmark_commit,
        tensor_profile_epoch,
        proof_latency_ms,
    }
}

/// A job submitted by a consumer with per-slice reference hashes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Job {
    pub job_id: String,
    pub buyer: String,
    pub slices: Vec<[u8; 32]>,
    /// Maximum price per compute unit the buyer is willing to pay.
    pub price_per_unit: u64,
    pub consumer_bond: u64,
    pub workloads: Vec<Workload>,
    /// Required hardware capability for the job.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub capability: scheduler::Capability,
    /// Unix timestamp by which the provider must deliver.
    pub deadline: u64,
    /// Priority for scheduling.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub priority: scheduler::Priority,
}

impl Job {
    fn contains_blocktorch_workload(&self) -> bool {
        self.workloads
            .iter()
            .any(|w| matches!(w, Workload::Inference(_)))
    }
}

/// Internal state for a matched job.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
struct JobState {
    job: Job,
    provider: String,
    provider_capability: scheduler::Capability,
    provider_bond: u64,
    price_per_unit: u64,
    fee_pct: u8,
    paid_slices: usize,
    completed: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    blocktorch_metadata: Option<BlockTorchWorkloadMetadata>,
    #[serde(default)]
    proof_latency_sum_ms: u64,
    #[serde(default)]
    proof_latency_count: u64,
}

/// In-memory market tracking offers and active jobs.
#[derive(Default)]
pub struct Market {
    offers: HashMap<String, Offer>,
    jobs: HashMap<String, JobState>,
    seen_jobs: std::collections::HashSet<String>,
    /// Pending compute settlement receipts for block inclusion
    pending_receipts: Vec<crate::ComputeReceipt>,
    /// Current block height for receipt emission
    current_block: u64,
}

impl Market {
    /// Create an empty market.
    pub fn new() -> Self {
        admission::record_available_shards(100);
        Self {
            offers: HashMap::new(),
            jobs: HashMap::new(),
            seen_jobs: std::collections::HashSet::new(),
            pending_receipts: Vec::new(),
            current_block: 0,
        }
    }

    /// Drain pending compute settlement receipts for block inclusion
    pub fn drain_receipts(&mut self) -> Vec<crate::ComputeReceipt> {
        #[cfg(feature = "telemetry")]
        crate::telemetry::receipts::set_receipt_drain_depth(self.pending_receipts.len());
        std::mem::take(&mut self.pending_receipts)
    }

    /// Set the current block height for receipt emission.
    /// Must be called before draining receipts during block construction.
    pub fn set_current_block(&mut self, block_height: u64) {
        self.current_block = block_height;
    }

    fn apply_blocktorch_metadata(
        &mut self,
        job_id: &str,
        metadata: Option<BlockTorchWorkloadMetadata>,
    ) {
        if let (Some(state), Some(meta)) = (self.jobs.get_mut(job_id), metadata) {
            record_blocktorch_metadata(&meta);
            state.blocktorch_metadata = Some(meta);
        }
    }

    fn sweep_overdue_jobs(&mut self) {
        for resolution in settlement::Settlement::sweep_overdue() {
            match &resolution.outcome {
                SlaResolutionKind::Violated { .. } => {
                    if let Some(state) = self.jobs.remove(&resolution.job_id) {
                        scheduler::record_failure(&state.provider);
                        if state.job.capability.accelerator.is_some() {
                            scheduler::record_accelerator_failure(&state.provider);
                            #[cfg(feature = "telemetry")]
                            crate::telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
                        }
                    }
                    scheduler::end_job(&resolution.job_id);
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::COMPUTE_JOB_TIMEOUT_TOTAL.inc();
                }
                SlaResolutionKind::Completed => {
                    if let Some(mut state) = self.jobs.remove(&resolution.job_id) {
                        let total_units: u64 = state.job.workloads.iter().map(|w| w.units()).sum();
                        let total_payment = total_units.saturating_mul(state.price_per_unit);
                        let verified = state.paid_slices == state.job.slices.len();

                        let proof_latency_ms = if state.proof_latency_count == 0 {
                            0
                        } else {
                            state.proof_latency_sum_ms / state.proof_latency_count
                        };
                        if let Some(snapshot) = tensor_profile::capture_tensor_profile_snapshot() {
                            #[cfg(feature = "telemetry")]
                            {
                                telemetry::receipts::set_orchard_alloc_free_delta(snapshot.delta);
                                for (label, delta) in snapshot.label_deltas.iter() {
                                    if *delta == 0 {
                                        continue;
                                    }
                                    telemetry::receipts::set_orchard_alloc_free_delta_detail(
                                        &resolution.job_id,
                                        label,
                                        *delta,
                                    );
                                }
                                telemetry::receipts::set_blocktorch_tensor_profile_epoch(Some(
                                    &snapshot.epoch,
                                ));
                            }
                            if let Some(meta) = state.blocktorch_metadata.as_mut() {
                                meta.tensor_profile_epoch = Some(snapshot.epoch.clone());
                            }
                        }

                        let blocktorch = if state.job.contains_blocktorch_workload() {
                            let meta = state
                                .blocktorch_metadata
                                .as_ref()
                                .expect("blocktorch job missing metadata");
                            Some(blocktorch_receipt_metadata(meta, proof_latency_ms))
                        } else {
                            None
                        };

                        self.pending_receipts.push(crate::ComputeReceipt {
                            job_id: resolution.job_id.clone(),
                            provider: state.provider.clone(),
                            compute_units: total_units,
                            payment: total_payment,
                            block_height: self.current_block,
                            verified,
                            blocktorch,
                            provider_signature: vec![],
                            signature_nonce: self.current_block,
                        });
                    }
                    scheduler::end_job(&resolution.job_id);
                }
                SlaResolutionKind::Cancelled { .. } => {
                    if let Some(_state) = self.jobs.remove(&resolution.job_id) {
                        // Cancelled jobs don't emit receipts
                    }
                    scheduler::end_job(&resolution.job_id);
                }
            }
        }
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
            offer.effective_reputation_multiplier(),
        );
        self.offers.insert(offer.job_id.clone(), offer);
        Ok(())
    }

    /// Submit a job from the consumer side, matching an existing offer.
    pub fn submit_job(&mut self, job: Job) -> Result<(), MarketError> {
        self.sweep_overdue_jobs();
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
                            .ensure_handle_for_label_values(&["capacity"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .inc();
                        telemetry::INDUSTRIAL_DEFERRED_TOTAL.inc();
                        return Err(MarketError::Capacity);
                    }
                    FairShare => {
                        telemetry::INDUSTRIAL_REJECTED_TOTAL
                            .ensure_handle_for_label_values(&["fair_share"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                            .inc();
                        return Err(MarketError::FairShare);
                    }
                    BurstExhausted => {
                        telemetry::INDUSTRIAL_REJECTED_TOTAL
                            .ensure_handle_for_label_values(&["burst_exhausted"])
                            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
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
        #[cfg(feature = "telemetry")]
        if self.seen_jobs.contains(&job.job_id) {
            telemetry::JOB_RESUBMITTED_TOTAL.inc();
        }
        price_board::record_price(
            FeeLane::Industrial,
            offer.price_per_unit,
            offer.effective_reputation_multiplier(),
        );
        #[cfg(feature = "telemetry")]
        let effective =
            (offer.price_per_unit as f64 * offer.effective_reputation_multiplier()).round() as u64;
        #[cfg(feature = "telemetry")]
        telemetry::SCHEDULER_EFFECTIVE_PRICE
            .ensure_handle_for_label_values(&[&offer.provider])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(effective as i64);
        let blocktorch_metadata = None;

        let state = JobState {
            job,
            provider: offer.provider.clone(),
            provider_capability: offer.capability.clone(),
            provider_bond: offer.provider_bond,
            price_per_unit: offer.price_per_unit,
            fee_pct: offer.fee_pct,
            paid_slices: 0,
            completed: false,
            blocktorch_metadata,
            proof_latency_sum_ms: 0,
            proof_latency_count: 0,
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time: {e}"))
            .as_secs();
        let expected = state.job.deadline.saturating_sub(now);
        scheduler::start_job_with_expected(
            &offer.job_id,
            &offer.provider,
            state.job.capability.clone(),
            state.job.priority,
            expected,
        );
        settlement::Settlement::track_sla(
            &state.job.job_id,
            &state.provider,
            &state.job.buyer,
            state.provider_bond,
            state.job.consumer_bond,
            state.job.deadline,
        );
        self.jobs.insert(state.job.job_id.clone(), state);
        self.seen_jobs.insert(offer.job_id);
        #[cfg(feature = "telemetry")]
        telemetry::INDUSTRIAL_ADMITTED_TOTAL.inc();
        Ok(())
    }

    /// Cancel an unmatched offer and return it.
    pub fn cancel_offer(&mut self, job_id: &str) -> Option<Offer> {
        self.offers.remove(job_id)
    }

    /// Cancel an in-flight job, returning both bonds.
    pub fn cancel_job(
        &mut self,
        job_id: &str,
        reason: scheduler::CancelReason,
    ) -> Option<(u64, u64)> {
        self.sweep_overdue_jobs();
        let state = self.jobs.remove(job_id)?;
        courier::cancel_job(job_id);
        let mut attempt = 0u32;
        let mut delay = Duration::from_millis(50);
        while attempt < 5 {
            if courier::release_resources(job_id) {
                break;
            }
            attempt += 1;
            thread::sleep(delay);
            delay *= 2;
        }
        if !scheduler::cancel_job(job_id, &state.provider, reason) {
            return None;
        }
        let outcome = match reason {
            scheduler::CancelReason::Provider | scheduler::CancelReason::ProviderFault => {
                SlaOutcome::Violated {
                    reason: reason.as_str(),
                    automated: false,
                }
            }
            _ => SlaOutcome::Cancelled {
                reason: reason.as_str(),
            },
        };
        let resolution = settlement::Settlement::resolve_sla(job_id, outcome);
        let mut provider_refund = state.provider_bond;
        let mut consumer_refund = state.job.consumer_bond;
        let refunded_by_resolution = resolution.as_ref().map_or(0, |res| res.refunded);
        if let Some(res) = &resolution {
            if let SlaResolutionKind::Violated { .. } = res.outcome {
                provider_refund = provider_refund.saturating_sub(res.burned);
            }
            if res.refunded > 0 {
                consumer_refund = res.refunded;
            }
        }
        if provider_refund > 0 {
            settlement::Settlement::accrue(&state.provider, "bond_refund", provider_refund);
        }
        if refunded_by_resolution == 0 && consumer_refund > 0 {
            settlement::Settlement::refund_split(&state.job.buyer, consumer_refund, 0);
        }
        Some((provider_refund, consumer_refund))
    }

    /// Verify a slice proof and record the payout.
    pub fn submit_slice(
        &mut self,
        job_id: &str,
        proof: ExecutionReceipt,
    ) -> Result<u64, &'static str> {
        use std::time::{SystemTime, UNIX_EPOCH};
        self.sweep_overdue_jobs();
        let state = self.jobs.get_mut(job_id).ok_or("unknown job")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time: {e}"))
            .as_secs();
        if now > state.job.deadline {
            scheduler::record_failure(&state.provider);
            if state.job.capability.accelerator.is_some() {
                scheduler::record_accelerator_failure(&state.provider);
                #[cfg(feature = "telemetry")]
                crate::telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
            }
            let _ = settlement::Settlement::resolve_sla(
                job_id,
                SlaOutcome::Violated {
                    reason: "deadline_missed",
                    automated: false,
                },
            );
            #[cfg(feature = "telemetry")]
            crate::telemetry::COMPUTE_JOB_TIMEOUT_TOTAL.inc();
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
            if state.job.capability.accelerator.is_some() {
                scheduler::record_accelerator_failure(&state.provider);
                #[cfg(feature = "telemetry")]
                crate::telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
            }
            return Err("reference mismatch");
        }
        let workload = &state.job.workloads[state.paid_slices];
        let (verified, proof_latency) = proof.verify_with_duration(workload);
        if !verified {
            scheduler::record_failure(&state.provider);
            if state.job.capability.accelerator.is_some() {
                scheduler::record_accelerator_failure(&state.provider);
                #[cfg(feature = "telemetry")]
                crate::telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
            }
            if matches!(workload, Workload::Snark(_)) {
                #[cfg(feature = "telemetry")]
                crate::telemetry::SNARK_FAIL_TOTAL.inc();
            }
            return Err("invalid proof");
        }
        if let Some(duration) = proof_latency {
            let latency_ms = duration.as_millis() as u64;
            state.proof_latency_sum_ms = state.proof_latency_sum_ms.saturating_add(latency_ms);
            state.proof_latency_count = state.proof_latency_count.saturating_add(1);
        }
        if let (Workload::Snark(_), Some(bundle)) = (workload, &proof.proof) {
            #[cfg(feature = "telemetry")]
            crate::telemetry::SNARK_VERIFICATIONS_TOTAL.inc();
            settlement::Settlement::record_proof(job_id, bundle.clone());
        }
        let slice_units = workload.units();
        let total_expected = slice_units
            .checked_mul(state.price_per_unit)
            .ok_or("payout overflow")?;
        if proof.payout != total_expected {
            scheduler::record_failure(&state.provider);
            if state.job.capability.accelerator.is_some() {
                scheduler::record_accelerator_failure(&state.provider);
                #[cfg(feature = "telemetry")]
                crate::telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
            }
            return Err("payout mismatch");
        }
        record_units_processed(slice_units);
        settlement::Settlement::accrue(&state.provider, "payout", proof.payout);
        state.paid_slices += 1;
        if state.paid_slices == state.job.slices.len() {
            state.completed = true;
        }
        Ok(proof.total())
    }

    /// Finalize a job and release bonds if complete.
    pub fn finalize_job(&mut self, job_id: &str) -> Option<(u64, u64)> {
        self.sweep_overdue_jobs();
        let state = self.jobs.get(job_id)?;
        if !state.completed {
            scheduler::record_failure(&state.provider);
            if state.job.capability.accelerator.is_some() {
                scheduler::record_accelerator_failure(&state.provider);
                #[cfg(feature = "telemetry")]
                crate::telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
            }
            return None;
        }
        let provider_id = state.provider.clone();
        let buyer_id = state.job.buyer.clone();
        let mut provider_refund = state.provider_bond;
        let mut consumer_refund = state.job.consumer_bond;
        let has_accel = state.job.capability.accelerator.is_some();
        let resolution = if let Some((expected, actual)) = scheduler::job_duration(job_id) {
            if expected > 0 && actual > expected {
                scheduler::record_failure(&provider_id);
                if has_accel {
                    scheduler::record_accelerator_failure(&provider_id);
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
                }
                #[cfg(feature = "telemetry")]
                crate::telemetry::COMPUTE_JOB_TIMEOUT_TOTAL.inc();
                settlement::Settlement::resolve_sla(
                    job_id,
                    SlaOutcome::Violated {
                        reason: "runtime_overage",
                        automated: false,
                    },
                )
            } else {
                settlement::Settlement::resolve_sla(job_id, SlaOutcome::Completed)
            }
        } else {
            settlement::Settlement::resolve_sla(job_id, SlaOutcome::Completed)
        };
        if let Some(res) = &resolution {
            if let SlaResolutionKind::Violated { .. } = res.outcome {
                provider_refund = provider_refund.saturating_sub(res.burned);
            }
            if res.refunded > 0 {
                consumer_refund = res.refunded;
            }
        }
        scheduler::record_success(&state.provider);
        if has_accel {
            scheduler::record_accelerator_success(&state.provider);
        }
        self.jobs.remove(job_id);
        scheduler::end_job(job_id);
        if provider_refund > 0 {
            settlement::Settlement::accrue(&provider_id, "bond_refund", provider_refund);
        }
        if resolution.as_ref().map_or(true, |res| res.refunded == 0) && consumer_refund > 0 {
            settlement::Settlement::refund_split(&buyer_id, consumer_refund, 0);
        }
        Some((provider_refund, consumer_refund))
    }

    /// Execute a job by submitting slice outputs and returning total payout.
    pub async fn execute_job(&mut self, job_id: &str) -> Result<u64, &'static str> {
        let (slices, workloads, price_per_unit, capability, provider_id) = {
            let state = self.jobs.get(job_id).ok_or("unknown job")?;
            (
                state.job.slices.clone(),
                state.job.workloads.clone(),
                state.price_per_unit,
                state.provider_capability.clone(),
                state.provider.clone(),
            )
        };
        let runner = WorkloadRunner::new();
        let mut handles = Vec::new();
        for (i, w) in workloads.iter().cloned().enumerate() {
            handles.push(runner.run(i, w));
        }
        let results = runtime::join_all(handles).await;
        let mut total = 0;
        for (expected, (run_output, w)) in slices
            .into_iter()
            .zip(results.into_iter().zip(workloads.into_iter()))
        {
            self.apply_blocktorch_metadata(job_id, run_output.blocktorch.clone());
            let output = run_output.output;
            let units = w.units();
            let proof_bundle = match &w {
                Workload::Snark(wasm) => {
                    let bundle = prove_with_capability(wasm, &output, &capability, &provider_id)
                        .map_err(|_| "snark_prover_failed")?;
                    Some(bundle)
                }
                _ => None,
            };
            let receipt = ExecutionReceipt {
                reference: expected,
                output,
                payout: units * price_per_unit,
                proof: proof_bundle,
            };
            total += self.submit_slice(job_id, receipt)?;
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

/// Global compute market instance for receipt emission
static COMPUTE_MARKET: concurrency::Lazy<concurrency::MutexT<Market>> =
    concurrency::Lazy::new(|| concurrency::mutex(Market::new()));

/// Access the global compute market instance
fn compute_market() -> concurrency::MutexGuard<'static, Market> {
    COMPUTE_MARKET.guard()
}

/// Set the current block height for compute receipt emission.
/// Must be called before draining receipts during block construction.
pub fn set_compute_current_block(block_height: u64) {
    compute_market().set_current_block(block_height);
}

/// Drain pending compute market receipts for block inclusion
pub fn drain_compute_receipts() -> Vec<crate::ComputeReceipt> {
    let receipts = compute_market().drain_receipts();

    // Record telemetry for drain operation
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::receipts::RECEIPT_DRAIN_OPERATIONS_TOTAL.inc();
        if !receipts.is_empty() {
            diagnostics::tracing::debug!(
                receipt_count = receipts.len(),
                market = "compute",
                "Drained compute receipts"
            );
        }
    }

    receipts
}

/// Drain compute SLA slash receipts for inclusion in block receipts.
pub fn drain_compute_slash_receipts(block_height: u64) -> Vec<crate::ComputeSlashReceipt> {
    settlement::Settlement::drain_slash_receipts(block_height)
}

fn prefer_gpu_backend(capability: &scheduler::Capability) -> bool {
    capability
        .gpu
        .as_ref()
        .map(|gpu| !gpu.is_empty())
        .unwrap_or(false)
        || capability
            .frameworks
            .iter()
            .any(|fw| fw.eq_ignore_ascii_case("cuda") || fw.eq_ignore_ascii_case("rocm"))
        || capability
            .accelerator
            .as_ref()
            .map(|acc| matches!(acc, Accelerator::Fpga | Accelerator::Tpu))
            .unwrap_or(false)
}

fn prove_with_capability(
    wasm: &[u8],
    output: &[u8],
    capability: &scheduler::Capability,
    provider: &str,
) -> Result<snark::ProofBundle, SnarkError> {
    if prefer_gpu_backend(capability) {
        match snark::prove_with_backend(wasm, output, SnarkBackend::Gpu) {
            Ok(bundle) => {
                scheduler::record_accelerator_success(provider);
                Ok(bundle)
            }
            Err(SnarkError::GpuUnavailable) => {
                scheduler::record_accelerator_failure(provider);
                snark::prove_with_backend(wasm, output, SnarkBackend::Cpu)
            }
            Err(err) => {
                scheduler::record_accelerator_failure(provider);
                Err(err)
            }
        }
    } else {
        snark::prove(wasm, output)
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
#[path = "tests/prover.rs"]
mod prover_benches;

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::hashing::blake3::Hasher;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static SETTLEMENT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct SettlementGuard {
        _lock: MutexGuard<'static, ()>,
        _dir: sys::tempfile::TempDir,
    }

    impl SettlementGuard {
        fn new() -> Self {
            let lock = SETTLEMENT_TEST_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let dir = sys::tempfile::tempdir().expect("settlement tempdir");
            let path = dir.path().join("settlement");
            let path_str = path.to_str().expect("settlement path str");
            settlement::Settlement::init(path_str, settlement::SettleMode::DryRun);
            Self {
                _lock: lock,
                _dir: dir,
            }
        }

        fn prefund(&self, account: &str, amount: u64) {
            settlement::Settlement::accrue(account, "test_prefund", amount);
        }
    }

    impl Drop for SettlementGuard {
        fn drop(&mut self) {
            settlement::Settlement::shutdown();
        }
    }

    #[test]
    fn offer_requires_bonds() {
        let offer = Offer {
            job_id: "job".into(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 10,
            price_per_unit: 5,
            fee_pct: 100,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        assert!(offer.validate().is_ok());
    }

    #[test]
    fn execution_receipt_verification() {
        let data = b"hello";
        let mut h = Hasher::new();
        h.update(data);
        let hash = *h.finalize().as_bytes();
        let receipt = ExecutionReceipt {
            reference: hash,
            output: hash,
            payout: 1,
            proof: None,
        };
        assert!(receipt.verify(&Workload::Transcode(data.to_vec())));
    }

    #[test]
    fn price_band_calc() {
        let bands = price_bands(&[1, 2, 3, 4]).unwrap_or_else(|| panic!("price bands"));
        assert_eq!(bands, (2, 3, 4));
    }

    #[test]
    fn courier_store_forward() {
        use crate::compute_market::courier::CourierStore;
        let dir = sys::tempfile::tempdir().unwrap_or_else(|e| panic!("create temp dir: {e}"));
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
        let settlement = SettlementGuard::new();
        settlement.prefund("prov", 1_000_000);
        settlement.prefund("buyer", 1_000_000);
        scheduler::reset_for_test();
        reset_units_processed_for_test();
        let mut market = Market::new();
        let job_id = "job1".to_string();
        let offer = Offer {
            job_id: job_id.clone(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 1,
            price_per_unit: 5,
            fee_pct: 100,
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
            priority: scheduler::Priority::Normal,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        let proof = ExecutionReceipt {
            reference: hash,
            output: hash,
            payout: 5,
            proof: None,
        };
        assert_eq!(
            market
                .submit_slice(&job_id, proof)
                .unwrap_or_else(|e| panic!("submit slice: {e}")),
            5
        );
        assert_eq!(total_units_processed(), 1);
        let bonds = market
            .finalize_job(&job_id)
            .unwrap_or_else(|| panic!("finalize job"));
        assert_eq!(bonds, (1, 1));
    }

    #[test]
    fn backlog_adjusts_price() {
        let settlement = SettlementGuard::new();
        settlement.prefund("prov", 1_000_000);
        settlement.prefund("buyer", 1_000_000);
        scheduler::reset_for_test();
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
            fee_pct: 100,
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
            priority: scheduler::Priority::Normal,
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
        let settlement = SettlementGuard::new();
        settlement.prefund("prov", 1_000_000);
        settlement.prefund("buyer", 1_000_000);
        scheduler::reset_for_test();
        let mut market = Market::new();
        let offer = Offer {
            job_id: "j2".into(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 1,
            price_per_unit: 5,
            fee_pct: 100,
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
            priority: scheduler::Priority::Normal,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        let bonds = market
            .cancel_job("j2", scheduler::CancelReason::Client)
            .unwrap_or_else(|| panic!("cancel job"));
        assert_eq!(bonds, (1, 1));
    }

    #[test]
    fn cancel_mid_execution_records_event() {
        let settlement = SettlementGuard::new();
        settlement.prefund("prov", 1_000_000);
        settlement.prefund("buyer", 1_000_000);
        scheduler::reset_for_test();
        let tmp = sys::tempfile::tempdir().unwrap();
        std::env::set_var(
            "TB_CANCEL_PATH",
            tmp.path().join("cancel.log").to_str().unwrap(),
        );
        let mut market = Market::new();
        let offer = Offer {
            job_id: "cj".into(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 2,
            price_per_unit: 5,
            fee_pct: 100,
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
            job_id: "cj".into(),
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
            priority: scheduler::Priority::Normal,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        let proof = ExecutionReceipt {
            reference: hash,
            output: hash,
            payout: 5,
            proof: None,
        };
        market
            .submit_slice("cj", proof)
            .unwrap_or_else(|e| panic!("submit slice: {e}"));
        let bonds = market
            .cancel_job("cj", scheduler::CancelReason::Client)
            .unwrap_or_else(|| panic!("cancel"));
        assert_eq!(bonds, (1, 1));
        let log = std::fs::read_to_string(tmp.path().join("cancel.log")).unwrap();
        assert!(log.contains("cj client"));
        assert!(scheduler::job_requirements("cj").is_none());
    }

    #[test]
    fn courier_cancel_stops_handoff() {
        let _settlement = SettlementGuard::new();
        scheduler::reset_for_test();
        courier::cancel_job("c1");
        assert!(courier::handoff_job("c1", "prov").is_err());
    }

    #[test]
    fn overdue_jobs_are_slashed_automatically() {
        use std::time::{Duration, SystemTime, UNIX_EPOCH};

        let settlement = SettlementGuard::new();
        settlement.prefund("prov", 10_000);
        settlement.prefund("buyer", 10_000);
        scheduler::reset_for_test();
        let mut market = Market::new();

        let offer = Offer {
            job_id: "auto1".into(),
            provider: "prov".into(),
            provider_bond: 5,
            consumer_bond: 5,
            units: 1,
            price_per_unit: 1,
            fee_pct: 0,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        market
            .post_offer(offer)
            .unwrap_or_else(|e| panic!("post offer: {e}"));

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let job = Job {
            job_id: "auto1".into(),
            buyer: "buyer".into(),
            slices: vec![[0u8; 32]],
            price_per_unit: 1,
            consumer_bond: 5,
            workloads: vec![Workload::Transcode(vec![])],
            capability: scheduler::Capability::default(),
            deadline: now + 1,
            priority: scheduler::Priority::Normal,
        };
        market.submit_job(job).unwrap();

        std::thread::sleep(Duration::from_secs(2));

        let offer2 = Offer {
            job_id: "auto2".into(),
            provider: "prov".into(),
            provider_bond: 5,
            consumer_bond: 5,
            units: 1,
            price_per_unit: 1,
            fee_pct: 0,
            capability: scheduler::Capability::default(),
            reputation: 0,
            reputation_multiplier: 1.0,
        };
        market
            .post_offer(offer2)
            .unwrap_or_else(|e| panic!("post offer 2: {e}"));
        let job2 = Job {
            job_id: "auto2".into(),
            buyer: "buyer".into(),
            slices: vec![[0u8; 32]],
            price_per_unit: 1,
            consumer_bond: 5,
            workloads: vec![Workload::Transcode(vec![])],
            capability: scheduler::Capability::default(),
            deadline: now + 60,
            priority: scheduler::Priority::Normal,
        };
        market.submit_job(job2).unwrap();

        assert!(market.jobs.contains_key("auto2"));
        assert!(!market.jobs.contains_key("auto1"));
        assert!(settlement::Settlement::balance("prov") < 10_000);
    }

    #[test]
    fn settlement_sweep_overdue_penalizes_and_records_resolution() {
        use settlement::SlaResolutionKind;
        use std::time::{SystemTime, UNIX_EPOCH};

        let settlement = SettlementGuard::new();
        settlement.prefund("prov", 1_000);
        settlement.prefund("buyer", 1_000);

        let past_deadline = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .saturating_sub(std::time::Duration::from_secs(5))
            .as_secs();

        settlement::Settlement::track_sla("sla-job", "prov", "buyer", 200, 100, past_deadline);

        let resolutions = settlement::Settlement::sweep_overdue();
        assert_eq!(resolutions.len(), 1);
        let resolution = &resolutions[0];
        assert_eq!(resolution.job_id, "sla-job");
        assert!(matches!(
            resolution.outcome,
            SlaResolutionKind::Violated { .. }
        ));
        assert!(resolution.burned >= 200);
        assert!(settlement::Settlement::balance("prov") <= 800);

        // second sweep should be idempotent once the queue is empty
        let follow_up = settlement::Settlement::sweep_overdue();
        assert!(follow_up.is_empty());
    }

    #[test]
    fn execute_job_path() {
        let settlement = SettlementGuard::new();
        settlement.prefund("prov", 1_000_000);
        settlement.prefund("buyer", 1_000_000);
        scheduler::reset_for_test();
        let mut market = Market::new();
        let job_id = "exec".to_string();
        let offer = Offer {
            job_id: job_id.clone(),
            provider: "prov".into(),
            provider_bond: 1,
            consumer_bond: 1,
            units: 1,
            price_per_unit: 2,
            fee_pct: 100,
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
            priority: scheduler::Priority::Normal,
        };
        market
            .submit_job(job)
            .unwrap_or_else(|e| panic!("submit job: {e}"));
        let total = runtime::block_on(market.execute_job(&job_id))
            .unwrap_or_else(|e| panic!("execute job: {e}"));
        assert_eq!(total, 2);
        let bonds = market
            .finalize_job(&job_id)
            .unwrap_or_else(|| panic!("finalize job"));
        assert_eq!(bonds, (1, 1));
    }
}

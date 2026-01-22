#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use diagnostics::{anyhow, Context, Result};
use foundation_lazy::sync::Lazy;
use foundation_serialization::json;
use foundation_serialization::ser::Serializer;
use foundation_serialization::Serialize;
use runtime::sync::CancellationToken;
use sys::tempfile;

use codec::{self, Codec as CodecProfile};
use coding::{Config as CodingConfig, RolloutConfig};
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use p2p_overlay::{
    Discovery, OverlayDiagnostics, OverlayResult, OverlayService, PeerId as OverlayPeerId,
    UptimeHandle, UptimeInfo, UptimeMetrics, UptimeStore,
};
use runtime::{self};
use storage_engine::{
    KeyValue, KeyValueBatch, KeyValueIterator, StorageError, StorageMetrics, StorageResult,
};
use the_block::compute_market::courier_store::ReceiptStore;
use the_block::compute_market::matcher;
use the_block::transaction::FeeLane;
use transport::{ProviderCapability, ProviderKind, ProviderMetadata};

/// Default output directory for the dependency fault harness.
pub static OUTPUT_ROOT: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("sim/output/dependency_fault"));

/// Runtime backend options.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum RuntimeBackendChoice {
    Inhouse,
    Stub,
}

/// Transport provider options.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum TransportBackendChoice {
    Quinn,
    S2n,
}

/// Overlay service choices.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum OverlayBackendChoice {
    Inhouse,
    Stub,
}

/// Storage engine options.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum StorageBackendChoice {
    LegacyRocksDb,
    Inhouse,
    Memory,
}

/// Coding backend toggle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum CodingBackendChoice {
    ReedSolomon,
    Xor,
}

/// Cryptography backend toggle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum CryptoBackendChoice {
    Dalek,
    Fallback,
}

/// Codec implementation toggle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum CodecBackendChoice {
    Json,
    Binary,
}

/// Targets that faults can be injected against.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize)]
pub enum FaultTarget {
    Runtime,
    Transport,
    Overlay,
    Storage,
    Coding,
    Crypto,
    Codec,
}

/// Fault types supported by the harness.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum FaultKind {
    Timeout,
    Panic,
}

/// Description of a single fault injection.
#[derive(Clone, Debug, Serialize)]
pub struct FaultSpec {
    pub target: FaultTarget,
    pub kind: FaultKind,
}

impl FromStr for FaultSpec {
    type Err = String;

    fn from_str(raw: &str) -> std::result::Result<Self, Self::Err> {
        let (target, kind) = raw
            .split_once(':')
            .ok_or_else(|| "expected <target>:<kind>".to_string())?;
        let target = target
            .parse::<FaultTarget>()
            .map_err(|err| format!("unknown fault target: {err}"))?;
        let kind = kind
            .parse::<FaultKind>()
            .map_err(|err| format!("unknown fault kind: {err}"))?;
        Ok(Self { target, kind })
    }
}

/// Selected backends for a simulation run.
#[derive(Clone, Debug, Serialize)]
pub struct BackendSelections {
    pub runtime: RuntimeBackendChoice,
    pub transport: TransportBackendChoice,
    pub overlay: OverlayBackendChoice,
    pub storage: StorageBackendChoice,
    pub coding: CodingBackendChoice,
    pub crypto: CryptoBackendChoice,
    pub codec: CodecBackendChoice,
}

impl Default for BackendSelections {
    fn default() -> Self {
        Self {
            runtime: RuntimeBackendChoice::Inhouse,
            transport: TransportBackendChoice::Quinn,
            overlay: OverlayBackendChoice::Inhouse,
            storage: StorageBackendChoice::LegacyRocksDb,
            coding: CodingBackendChoice::ReedSolomon,
            crypto: CryptoBackendChoice::Dalek,
            codec: CodecBackendChoice::Binary,
        }
    }
}

/// Parameters for running the dependency fault simulation.
#[derive(Clone, Debug)]
pub struct SimulationRequest {
    pub selections: BackendSelections,
    pub faults: Vec<FaultSpec>,
    pub duration: Duration,
    pub iterations: u32,
    pub output_root: PathBuf,
    pub label: Option<String>,
    pub persist_logs: bool,
}

impl Default for SimulationRequest {
    fn default() -> Self {
        Self {
            selections: BackendSelections::default(),
            faults: Vec::new(),
            duration: Duration::from_secs(2),
            iterations: 1,
            output_root: OUTPUT_ROOT.clone(),
            label: None,
            persist_logs: true,
        }
    }
}

/// Metrics captured for a single scenario run.
#[derive(Clone, Debug, Serialize)]
pub struct ScenarioMetrics {
    pub scenario: String,
    pub iteration: u32,
    pub runtime_backend: String,
    pub transport_backend: String,
    pub overlay_backend: String,
    pub storage_backend: String,
    pub coding_backend: String,
    pub crypto_backend: String,
    pub codec_backend: String,
    pub faults: Vec<String>,
    pub receipts_persisted: u64,
    pub match_loop_errors: u64,
    pub transport_success: u64,
    pub transport_failures: u64,
    pub overlay_peers: usize,
    pub overlay_claims: u64,
    pub overlay_failures: u64,
    pub storage_ops: u64,
    pub storage_failures: u64,
    pub coding_bytes: u64,
    pub coding_failures: u64,
    pub crypto_ops: u64,
    pub crypto_failures: u64,
    pub codec_ops: u64,
    pub codec_failures: u64,
    pub rpc_latency_ms: f64,
    pub rpc_failures: u64,
    pub consensus_difficulty: u64,
    pub duration_secs: f64,
    pub fault_events: Vec<String>,
}

impl ScenarioMetrics {
    fn new(
        name: impl Into<String>,
        iteration: u32,
        selections: &BackendSelections,
        faults: &[FaultSpec],
    ) -> Self {
        let faults = faults
            .iter()
            .map(|f| format!("{:?}:{:?}", f.target, f.kind))
            .collect();
        Self {
            scenario: name.into(),
            iteration,
            runtime_backend: selections.runtime.as_str().to_string(),
            transport_backend: selections.transport.as_str().to_string(),
            overlay_backend: selections.overlay.as_str().to_string(),
            storage_backend: selections.storage.as_str().to_string(),
            coding_backend: selections.coding.as_str().to_string(),
            crypto_backend: selections.crypto.as_str().to_string(),
            codec_backend: selections.codec.as_str().to_string(),
            faults,
            receipts_persisted: 0,
            match_loop_errors: 0,
            transport_success: 0,
            transport_failures: 0,
            overlay_peers: 0,
            overlay_claims: 0,
            overlay_failures: 0,
            storage_ops: 0,
            storage_failures: 0,
            coding_bytes: 0,
            coding_failures: 0,
            crypto_ops: 0,
            crypto_failures: 0,
            codec_ops: 0,
            codec_failures: 0,
            rpc_latency_ms: 0.0,
            rpc_failures: 0,
            consensus_difficulty: 0,
            duration_secs: 0.0,
            fault_events: Vec::new(),
        }
    }
}

/// File artifacts emitted for each scenario.
#[derive(Clone, Debug)]
pub struct ScenarioReport {
    pub metrics: ScenarioMetrics,
    pub metrics_path: PathBuf,
    pub summary_path: PathBuf,
    pub log_path: Option<PathBuf>,
}

impl Serialize for ScenarioReport {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use foundation_serialization::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ScenarioReport", 4)?;
        state.serialize_field("metrics", &self.metrics)?;
        state.serialize_field("metrics_path", &self.metrics_path.display().to_string())?;
        state.serialize_field("summary_path", &self.summary_path.display().to_string())?;
        state.serialize_field(
            "log_path",
            &self.log_path.as_ref().map(|p| p.display().to_string()),
        )?;
        state.end()
    }
}

/// Summary for the entire simulation run.
#[derive(Clone, Debug)]
pub struct SimulationSummary {
    pub base_dir: PathBuf,
    pub reports: Vec<ScenarioReport>,
}

impl Serialize for SimulationSummary {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use foundation_serialization::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("SimulationSummary", 2)?;
        state.serialize_field("base_dir", &self.base_dir.display().to_string())?;
        state.serialize_field("reports", &self.reports)?;
        state.end()
    }
}

/// Run the dependency fault simulation and return the summary.
pub fn run_simulation(request: &SimulationRequest) -> Result<SimulationSummary> {
    configure_runtime(&request.selections.runtime);
    fs::create_dir_all(&request.output_root)
        .with_context(|| format!("create output root {}", request.output_root.display()))?;
    let run_dir = make_run_dir(&request.output_root, request.label.as_deref())?;
    let mut reports = Vec::new();

    let baseline_faults: Vec<FaultSpec> = Vec::new();
    for iteration in 0..request.iterations {
        // Baseline scenario
        let baseline_dir = run_dir.join(format!("baseline_{iteration}"));
        fs::create_dir_all(&baseline_dir)?;
        let mut metrics =
            ScenarioMetrics::new("baseline", iteration, &request.selections, &baseline_faults);
        reports.push(execute_scenario(
            &baseline_dir,
            &request.selections,
            &baseline_faults,
            request.duration,
            &mut metrics,
            request.persist_logs,
        )?);

        if !request.faults.is_empty() {
            let fault_dir = run_dir.join(format!("faulted_{iteration}"));
            fs::create_dir_all(&fault_dir)?;
            let mut metrics =
                ScenarioMetrics::new("faulted", iteration, &request.selections, &request.faults);
            reports.push(execute_scenario(
                &fault_dir,
                &request.selections,
                &request.faults,
                request.duration,
                &mut metrics,
                request.persist_logs,
            )?);
        }
    }

    Ok(SimulationSummary {
        base_dir: run_dir,
        reports,
    })
}

fn configure_runtime(choice: &RuntimeBackendChoice) {
    let env_value = choice.as_env();
    std::env::set_var("TB_RUNTIME_BACKEND", env_value);
    let handle = runtime::handle();
    if handle.backend_name() != env_value {
        eprintln!(
            "requested runtime backend {env_value} but active backend is {}; feature flags may be missing",
            handle.backend_name()
        );
    }
}

fn make_run_dir(root: &Path, label: Option<&str>) -> Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
    let dir = match label {
        Some(label) => root.join(format!("{ts}_{label}")),
        None => root.join(format!("{ts}")),
    };
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

struct FaultInjector {
    faults: HashMap<FaultTarget, FaultKind>,
}

impl FaultInjector {
    fn new(faults: &[FaultSpec]) -> Self {
        let mut map = HashMap::new();
        for fault in faults {
            map.insert(fault.target, fault.kind);
        }
        Self { faults: map }
    }

    fn get(&self, target: FaultTarget) -> Option<FaultKind> {
        self.faults.get(&target).copied()
    }
}

fn execute_scenario(
    dir: &Path,
    selections: &BackendSelections,
    faults: &[FaultSpec],
    duration: Duration,
    metrics: &mut ScenarioMetrics,
    persist_logs: bool,
) -> Result<ScenarioReport> {
    let injector = FaultInjector::new(faults);
    let mut logs = Vec::new();
    let start = Instant::now();

    run_match_loop(duration, selections, &injector, metrics, &mut logs)?;
    run_transport_probe(selections, &injector, metrics, &mut logs)?;
    run_overlay_probe(selections, &injector, metrics, &mut logs)?;
    run_storage_probe(selections, &injector, metrics, &mut logs)?;
    run_coding_probe(selections, &injector, metrics, &mut logs)?;
    run_crypto_probe(selections, &injector, metrics, &mut logs)?;
    run_codec_probe(selections, &injector, metrics, &mut logs)?;
    run_rpc_probe(&injector, metrics, &mut logs)?;

    metrics.duration_secs = start.elapsed().as_secs_f64();

    let metrics_path = dir.join("metrics.json");
    let summary_path = dir.join("summary.md");
    let log_path = if persist_logs {
        Some(dir.join("events.log"))
    } else {
        None
    };

    let json = json::to_vec_pretty(metrics).map_err(|err| anyhow!(err))?;
    fs::write(&metrics_path, json)?;
    let mut summary = File::create(&summary_path)?;
    write_summary(&mut summary, metrics)?;

    if let Some(path) = &log_path {
        let mut file = File::create(path)?;
        for entry in logs {
            writeln!(file, "{entry}")?;
        }
    }

    Ok(ScenarioReport {
        metrics: metrics.clone(),
        metrics_path,
        summary_path,
        log_path,
    })
}

fn write_summary(out: &mut dyn Write, metrics: &ScenarioMetrics) -> Result<()> {
    writeln!(
        out,
        "# Scenario: {} (iteration {})",
        metrics.scenario, metrics.iteration
    )?;
    writeln!(out)?;
    writeln!(out, "* Runtime backend: {}", metrics.runtime_backend)?;
    writeln!(out, "* Transport backend: {}", metrics.transport_backend)?;
    writeln!(out, "* Overlay backend: {}", metrics.overlay_backend)?;
    writeln!(out, "* Storage backend: {}", metrics.storage_backend)?;
    writeln!(out, "* Coding backend: {}", metrics.coding_backend)?;
    writeln!(out, "* Crypto backend: {}", metrics.crypto_backend)?;
    writeln!(out, "* Codec backend: {}", metrics.codec_backend)?;
    if metrics.faults.is_empty() {
        writeln!(out, "* Faults: none")?;
    } else {
        writeln!(out, "* Faults: {}", metrics.faults.join(", "))?;
    }
    writeln!(out)?;
    writeln!(out, "## Outcomes")?;
    writeln!(out, "- Receipts persisted: {}", metrics.receipts_persisted)?;
    writeln!(out, "- Match loop errors: {}", metrics.match_loop_errors)?;
    writeln!(
        out,
        "- Transport success/failures: {}/{}",
        metrics.transport_success, metrics.transport_failures
    )?;
    writeln!(out, "- Overlay peers tracked: {}", metrics.overlay_peers)?;
    writeln!(out, "- Overlay claims issued: {}", metrics.overlay_claims)?;
    writeln!(out, "- Overlay failures: {}", metrics.overlay_failures)?;
    writeln!(
        out,
        "- Storage ops/failures: {}/{}",
        metrics.storage_ops, metrics.storage_failures
    )?;
    writeln!(out, "- Coding bytes processed: {}", metrics.coding_bytes)?;
    writeln!(out, "- Coding failures: {}", metrics.coding_failures)?;
    writeln!(
        out,
        "- Crypto ops/failures: {}/{}",
        metrics.crypto_ops, metrics.crypto_failures
    )?;
    writeln!(
        out,
        "- Codec ops/failures: {}/{}",
        metrics.codec_ops, metrics.codec_failures
    )?;
    writeln!(out, "- RPC latency (ms): {:.2}", metrics.rpc_latency_ms)?;
    writeln!(out, "- RPC failures: {}", metrics.rpc_failures)?;
    writeln!(
        out,
        "- Consensus difficulty: {}",
        metrics.consensus_difficulty
    )?;
    writeln!(out, "- Runtime duration (s): {:.3}", metrics.duration_secs)?;
    if !metrics.fault_events.is_empty() {
        writeln!(out)?;
        writeln!(out, "## Fault Events")?;
        for event in &metrics.fault_events {
            writeln!(out, "- {event}")?;
        }
    }
    Ok(())
}

fn run_match_loop(
    duration: Duration,
    selections: &BackendSelections,
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    let mut lanes = sample_lanes();
    if matches!(selections.coding, CodingBackendChoice::Xor) {
        // tighten fairness window to stress alternate coding configuration
        for lane in &mut lanes {
            lane.metadata.fairness_window = Duration::from_millis(5);
        }
    }
    matcher::seed_orders(lanes).map_err(|e| anyhow!("{e}"))?;
    let tempdir = tempfile::Builder::new()
        .prefix("dependency-fault-receipts")
        .tempdir()
        .map_err(|e| anyhow!("{e}"))?;
    let store = ReceiptStore::open(tempdir.path().to_str().unwrap());
    let stop = CancellationToken::new();
    let fault = injector.get(FaultTarget::Runtime);
    if fault == Some(FaultKind::Panic) {
        metrics
            .fault_events
            .push("runtime panic injected during match loop".into());
        let res = std::panic::catch_unwind(|| {
            runtime::spawn(matcher::match_loop(store.clone(), true, stop.clone()));
        });
        if res.is_err() {
            metrics.match_loop_errors += 1;
            logs.push("match loop panicked before starting".into());
            return Ok(());
        }
    }
    let handle = runtime::spawn(matcher::match_loop(store.clone(), true, stop.clone()));
    runtime::block_on(async {
        if fault == Some(FaultKind::Timeout) {
            metrics
                .fault_events
                .push("runtime timeout injected during match loop".into());
            runtime::sleep(Duration::from_millis(10)).await;
        } else {
            runtime::sleep(duration).await;
        }
        stop.cancel();
    });
    match runtime::block_on(async { handle.await }) {
        Ok(_) => {}
        Err(err) => {
            metrics.match_loop_errors += 1;
            logs.push(format!("match loop join error: {err}"));
        }
    }
    metrics.receipts_persisted = store.len().unwrap_or(0) as u64;
    Ok(())
}

fn sample_lanes() -> Vec<matcher::LaneSeed> {
    use matcher::{Ask, Bid, LaneMetadata, LaneSeed};
    let consumer = LaneSeed {
        lane: FeeLane::Consumer,
        bids: (0..4)
            .map(|i| Bid {
                job_id: format!("consumer-job-{i}"),
                buyer: format!("buyer-{i}"),
                price: 10 + i as u64,
                lane: FeeLane::Consumer,
            })
            .collect(),
        asks: (0..4)
            .map(|i| Ask {
                job_id: format!("consumer-ask-{i}"),
                provider: format!("provider-{i}"),
                price: 8 + i as u64,
                lane: FeeLane::Consumer,
            })
            .collect(),
        metadata: LaneMetadata::default(),
    };
    let industrial = LaneSeed {
        lane: FeeLane::Industrial,
        bids: (0..3)
            .map(|i| Bid {
                job_id: format!("industrial-job-{i}"),
                buyer: format!("industrial-buyer-{i}"),
                price: 20 + i as u64,
                lane: FeeLane::Industrial,
            })
            .collect(),
        asks: (0..3)
            .map(|i| Ask {
                job_id: format!("industrial-ask-{i}"),
                provider: format!("industrial-provider-{i}"),
                price: 19 + i as u64,
                lane: FeeLane::Industrial,
            })
            .collect(),
        metadata: LaneMetadata::default(),
    };
    vec![consumer, industrial]
}

fn run_transport_probe(
    selections: &BackendSelections,
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    let provider = match selections.transport {
        TransportBackendChoice::Quinn => ProviderKind::Quinn,
        TransportBackendChoice::S2n => ProviderKind::S2nQuic,
    };
    let meta = ProviderMetadata {
        kind: provider,
        id: provider.id(),
        capabilities: &[
            ProviderCapability::CertificateRotation,
            ProviderCapability::TelemetryCallbacks,
        ],
    };
    let fault = injector.get(FaultTarget::Transport);
    if let Some(kind) = fault {
        match kind {
            FaultKind::Timeout => metrics
                .fault_events
                .push("transport timeout injected".into()),
            FaultKind::Panic => metrics.fault_events.push("transport panic injected".into()),
        }
    }
    let mut transport = SimulatedTransport::new(meta, fault);
    for attempt in 0..3 {
        match runtime::block_on(transport.connect()) {
            Ok(_) => metrics.transport_success += 1,
            Err(err) => {
                metrics.transport_failures += 1;
                logs.push(format!("transport attempt {attempt} failed: {err}"));
            }
        }
    }
    Ok(())
}

fn run_overlay_probe(
    selections: &BackendSelections,
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    let fault = injector.get(FaultTarget::Overlay);
    if let Some(kind) = fault {
        match kind {
            FaultKind::Timeout => metrics.fault_events.push("overlay timeout injected".into()),
            FaultKind::Panic => metrics.fault_events.push("overlay panic injected".into()),
        }
    }
    let mut overlay = SimulatedOverlay::new(selections.overlay, fault);
    overlay.bootstrap()?;
    metrics.overlay_peers = overlay.diagnostics()?.active_peers;
    metrics.overlay_claims = overlay.claims();
    metrics.overlay_failures = overlay.failures();
    logs.extend(overlay.take_logs());
    Ok(())
}

fn run_storage_probe(
    selections: &BackendSelections,
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    let fault = injector.get(FaultTarget::Storage);
    if let Some(kind) = fault {
        match kind {
            FaultKind::Timeout => metrics.fault_events.push("storage timeout injected".into()),
            FaultKind::Panic => metrics.fault_events.push("storage panic injected".into()),
        }
    }
    let storage = SimulatedStorage::new(selections.storage, fault);
    storage.ensure_cf("receipts").map_err(|e| anyhow!("{e}"))?;
    for idx in 0..4 {
        let key = format!("key-{idx}");
        let value = format!("value-{idx}");
        match storage.put_bytes("receipts", key.as_bytes(), value.as_bytes()) {
            Ok(()) => metrics.storage_ops += 1,
            Err(err) => {
                metrics.storage_failures += 1;
                logs.push(format!("storage put failed: {err}"));
            }
        }
    }
    Ok(())
}

fn run_coding_probe(
    selections: &BackendSelections,
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    let mut config = CodingConfig::default();
    if matches!(selections.coding, CodingBackendChoice::Xor) {
        config.rollout = RolloutConfig {
            allow_fallback_coder: true,
            allow_fallback_compressor: true,
            require_emergency_switch: false,
            emergency_switch_env: None,
        };
        config.erasure.algorithm = "xor".into();
        config.erasure.data_shards = 4;
        config.erasure.parity_shards = 1;
        config.compression.algorithm = "rle".into();
    }
    let fault = injector.get(FaultTarget::Coding);
    let payload = vec![0xAA; 4096];
    if fault == Some(FaultKind::Panic) {
        metrics.fault_events.push("coding panic injected".into());
        let res = std::panic::catch_unwind(|| {
            let coder = config.erasure_coder().expect("erasure coder");
            let _ = coder.encode(&payload).expect("encode");
        });
        if res.is_err() {
            metrics.coding_failures += 1;
            logs.push("coding panic triggered".into());
            return Ok(());
        }
    }
    let coder = config
        .erasure_coder()
        .map_err(|err| anyhow!("erasure coder: {err}"))?;
    let compressor = config
        .compressor()
        .map_err(|err| anyhow!("compressor: {err}"))?;
    let compressed = compressor.compress(&payload);
    match compressed {
        Ok(bytes) => {
            metrics.coding_bytes += bytes.len() as u64;
            match coder.encode(&bytes) {
                Ok(batch) => metrics.coding_bytes += batch.shards.len() as u64,
                Err(err) => {
                    metrics.coding_failures += 1;
                    logs.push(format!("erasure encode failed: {err}"));
                }
            }
        }
        Err(err) => {
            metrics.coding_failures += 1;
            logs.push(format!("compression failed: {err}"));
        }
    }
    if fault == Some(FaultKind::Timeout) {
        metrics.fault_events.push("coding timeout injected".into());
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn run_crypto_probe(
    _selections: &BackendSelections,
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    let fault = injector.get(FaultTarget::Crypto);
    if fault == Some(FaultKind::Timeout) {
        metrics.fault_events.push("crypto timeout injected".into());
        std::thread::sleep(Duration::from_millis(15));
    }
    let signing_key = SigningKey::generate(&mut rand::thread_rng());
    let verifying_key: VerifyingKey = signing_key.verifying_key();
    let payload = b"dependency fault harness";
    if fault == Some(FaultKind::Panic) {
        metrics.fault_events.push("crypto panic injected".into());
        let res = std::panic::catch_unwind(|| signing_key.sign(payload));
        if res.is_err() {
            metrics.crypto_failures += 1;
            logs.push("crypto panic triggered".into());
            return Ok(());
        }
    }
    let signature: Signature = signing_key.sign(payload);
    metrics.crypto_ops += 1;
    if verifying_key.verify(payload, &signature).is_err() {
        metrics.crypto_failures += 1;
        logs.push("signature verification failed".into());
    }
    Ok(())
}

fn run_codec_probe(
    selections: &BackendSelections,
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    #[derive(Serialize)]
    struct Payload<'a> {
        label: &'a str,
        value: u64,
    }
    let payload = Payload {
        label: "dependency_fault",
        value: 42,
    };
    let profile = match selections.codec {
        CodecBackendChoice::Json => CodecProfile::Json(codec::profiles::json::profile()),
        CodecBackendChoice::Binary => CodecProfile::Binary(codec::profiles::transaction::profile()),
    };
    let fault = injector.get(FaultTarget::Codec);
    if fault == Some(FaultKind::Panic) {
        metrics.fault_events.push("codec panic injected".into());
        let res = std::panic::catch_unwind(|| serialize_with_profile(profile, &payload).unwrap());
        if res.is_err() {
            metrics.codec_failures += 1;
            logs.push("codec panic triggered".into());
            return Ok(());
        }
    }
    let bytes = match serialize_with_profile(profile, &payload) {
        Ok(bytes) => bytes,
        Err(err) => {
            metrics.codec_failures += 1;
            logs.push(format!("serialization failed: {err}"));
            return Ok(());
        }
    };
    metrics.codec_ops += 1;
    if fault == Some(FaultKind::Timeout) {
        metrics.fault_events.push("codec timeout injected".into());
        std::thread::sleep(Duration::from_millis(10));
    }
    if matches!(selections.codec, CodecBackendChoice::Json) {
        // ensure we can read back textual payloads
        let text = String::from_utf8(bytes.clone()).map_err(|err| anyhow!("{err}"))?;
        if !text.contains("dependency_fault") {
            metrics.codec_failures += 1;
            logs.push("json serialization missing label".into());
        }
    }
    Ok(())
}

fn serialize_with_profile<T: Serialize>(
    profile: CodecProfile,
    value: &T,
) -> codec::Result<Vec<u8>> {
    codec::serialize(profile, value)
}

fn run_rpc_probe(
    injector: &FaultInjector,
    metrics: &mut ScenarioMetrics,
    logs: &mut Vec<String>,
) -> Result<()> {
    use the_block::rpc::consensus;
    let fault = injector.get(FaultTarget::Runtime);
    if fault == Some(FaultKind::Timeout) {
        metrics.rpc_failures += 1;
        metrics.fault_events.push("rpc timeout injected".into());
        logs.push("rpc timeout injected".into());
        return Ok(());
    }
    let blockchain = Arc::new(Mutex::new(the_block::Blockchain::default()));
    let start = Instant::now();
    let response = consensus::difficulty(&blockchain);
    metrics.rpc_latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    metrics.consensus_difficulty = response.difficulty;
    Ok(())
}

struct SimulatedTransport {
    meta: ProviderMetadata,
    fault: Option<FaultKind>,
    attempts: u64,
}

impl SimulatedTransport {
    fn new(meta: ProviderMetadata, fault: Option<FaultKind>) -> Self {
        Self {
            meta,
            fault,
            attempts: 0,
        }
    }

    async fn connect(&mut self) -> Result<(), SimulatedTransportError> {
        self.attempts += 1;
        if let Some(fault) = self.fault {
            match fault {
                FaultKind::Timeout => {
                    if self.attempts % 2 == 0 {
                        return Err(SimulatedTransportError::Timeout(self.meta.id.to_string()));
                    }
                }
                FaultKind::Panic => {
                    panic!("transport panic injected for {}", self.meta.id);
                }
            }
        }
        runtime::sleep(Duration::from_millis(5)).await;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum SimulatedTransportError {
    #[error("timeout connecting via {0}")]
    Timeout(String),
}

struct SimulatedOverlay {
    backend: OverlayBackendChoice,
    fault: Option<FaultKind>,
    peers: HashSet<String>,
    claims: u64,
    failures: u64,
    logs: Vec<String>,
}

impl SimulatedOverlay {
    fn new(backend: OverlayBackendChoice, fault: Option<FaultKind>) -> Self {
        Self {
            backend,
            fault,
            peers: HashSet::new(),
            claims: 0,
            failures: 0,
            logs: Vec::new(),
        }
    }

    fn bootstrap(&mut self) -> Result<()> {
        if let Some(FaultKind::Panic) = self.fault {
            self.failures += 1;
            self.logs.push("overlay panic injected".into());
            return Ok(());
        }
        let service = MockOverlayService::new(self.backend, self.fault);
        let local = MockPeerId("local".into());
        let mut discovery = service.discovery(local.clone());
        for idx in 0..5 {
            let peer = MockPeerId(format!("peer-{idx}"));
            discovery.add_peer(peer.clone(), format!("addr-{idx}"));
            self.peers.insert(peer.0);
        }
        let uptime = service.uptime();
        for peer in self.peers.iter().cloned() {
            uptime.note_seen(MockPeerId(peer));
        }
        self.claims = service.claims();
        if self.fault == Some(FaultKind::Timeout) {
            self.logs.push("overlay timeout injected".into());
            self.failures += 1;
        }
        Ok(())
    }

    fn diagnostics(&self) -> Result<OverlayDiagnostics> {
        Ok(OverlayDiagnostics {
            label: match self.backend {
                OverlayBackendChoice::Inhouse => "inhouse",
                OverlayBackendChoice::Stub => "stub",
            },
            active_peers: self.peers.len(),
            persisted_peers: self.peers.len(),
            database_path: None,
            persist_attempts: 0,
            persist_successes: 0,
            persist_failures: 0,
        })
    }

    fn claims(&self) -> u64 {
        self.claims
    }

    fn failures(&self) -> u64 {
        self.failures
    }

    fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}

struct MockOverlayService {
    backend: OverlayBackendChoice,
    #[allow(dead_code)] // Used for fault injection in future tests
    fault: Option<FaultKind>,
    store: Arc<MockUptimeStore>,
}

impl MockOverlayService {
    fn new(backend: OverlayBackendChoice, fault: Option<FaultKind>) -> Self {
        Self {
            backend,
            fault,
            store: Arc::new(MockUptimeStore::default()),
        }
    }

    fn claims(&self) -> u64 {
        self.store.claims()
    }
}

#[derive(Clone, Default)]
struct MockUptimeStore {
    peers: Arc<Mutex<HashMap<MockPeerId, UptimeInfo>>>,
    claims: Arc<Mutex<u64>>,
}

impl UptimeStore<MockPeerId> for MockUptimeStore {
    fn with_map<R>(&self, f: impl FnOnce(&mut HashMap<MockPeerId, UptimeInfo>) -> R) -> R {
        let mut guard = self.peers.lock().unwrap();
        f(&mut guard)
    }
}

impl MockUptimeStore {
    fn note_claim(&self) {
        let mut guard = self.claims.lock().unwrap();
        *guard += 1;
    }

    fn claims(&self) -> u64 {
        *self.claims.lock().unwrap()
    }
}

struct MockUptimeHandle {
    store: Arc<MockUptimeStore>,
}

impl UptimeHandle for MockUptimeHandle {
    type Peer = MockPeerId;

    fn note_seen(&self, peer: Self::Peer) {
        self.store.with_map(|map| {
            map.entry(peer).or_insert_with(UptimeInfo::default).total += 1;
        });
    }

    fn eligible(&self, peer: &Self::Peer, threshold: u64, _epoch: u64) -> bool {
        self.store.with_map(|map| {
            map.get(peer)
                .map(|info| info.total >= threshold)
                .unwrap_or(false)
        })
    }

    fn claim(&self, peer: Self::Peer, threshold: u64, _epoch: u64, reward: u64) -> Option<u64> {
        if self.eligible(&peer, threshold, 0) {
            self.store.note_claim();
            Some(reward)
        } else {
            None
        }
    }
}

impl MockUptimeHandle {
    #[allow(dead_code)] // Helper for future test assertions
    fn claims(&self) -> u64 {
        self.store.claims()
    }
}

impl UptimeMetrics for MockUptimeStore {}

impl OverlayService for MockOverlayService {
    type Peer = MockPeerId;
    type Address = String;

    fn peer_from_bytes(&self, bytes: &[u8]) -> OverlayResult<Self::Peer> {
        Ok(MockPeerId(
            String::from_utf8(bytes.to_vec()).map_err(|e| anyhow!("{e}"))?,
        ))
    }

    fn peer_to_bytes(&self, peer: &Self::Peer) -> Vec<u8> {
        peer.0.as_bytes().to_vec()
    }

    fn discovery(
        &self,
        _local: Self::Peer,
    ) -> Box<dyn Discovery<Peer = Self::Peer, Address = Self::Address> + Send> {
        Box::new(MockDiscovery::default())
    }

    fn uptime(&self) -> Arc<dyn UptimeHandle<Peer = Self::Peer>> {
        Arc::new(MockUptimeHandle {
            store: self.store.clone(),
        })
    }

    fn diagnostics(&self) -> OverlayResult<OverlayDiagnostics> {
        Ok(OverlayDiagnostics {
            label: match self.backend {
                OverlayBackendChoice::Inhouse => "inhouse",
                OverlayBackendChoice::Stub => "stub",
            },
            active_peers: 0,
            persisted_peers: 0,
            database_path: None,
            persist_attempts: 0,
            persist_successes: 0,
            persist_failures: 0,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
struct MockPeerId(String);

impl OverlayPeerId for MockPeerId {
    fn from_bytes(bytes: &[u8]) -> OverlayResult<Self>
    where
        Self: Sized,
    {
        Ok(Self(
            String::from_utf8(bytes.to_vec()).map_err(|e| anyhow!("{e}"))?,
        ))
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.as_bytes().to_vec()
    }
}

#[derive(Default)]
struct MockDiscovery {
    peers: HashSet<MockPeerId>,
}

impl Discovery for MockDiscovery {
    type Peer = MockPeerId;
    type Address = String;

    fn add_peer(&mut self, peer: Self::Peer, _address: Self::Address) {
        self.peers.insert(peer);
    }

    fn has_peer(&self, peer: &Self::Peer) -> bool {
        self.peers.contains(peer)
    }

    fn persist(&self) {}
}

struct SimulatedStorage {
    backend: StorageBackendChoice,
    fault: Option<FaultKind>,
    inner: InMemoryStore,
}

impl SimulatedStorage {
    fn new(backend: StorageBackendChoice, fault: Option<FaultKind>) -> Self {
        Self {
            backend,
            fault,
            inner: InMemoryStore::default(),
        }
    }
}

impl KeyValue for SimulatedStorage {
    type Batch = MemoryBatch;
    type Iter = MemoryIter;

    fn open(_path: &str) -> StorageResult<Self>
    where
        Self: Sized,
    {
        Ok(Self::new(StorageBackendChoice::Memory, None))
    }

    fn flush_wal(&self) -> StorageResult<()> {
        Ok(())
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        if let Some(FaultKind::Panic) = self.fault {
            return Err(StorageError::backend(format!("{cf} panic")));
        }
        self.inner.ensure_cf(cf)
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.inner.get(cf, key)
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.inner.put(cf, key, value)
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        if let Some(FaultKind::Timeout) = self.fault {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.inner.put_bytes(cf, key, value)
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.inner.delete(cf, key)
    }

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<Self::Iter> {
        self.inner.prefix_iterator(cf, prefix)
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        self.inner.list_cfs()
    }

    fn make_batch(&self) -> Self::Batch {
        MemoryBatch::default()
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        self.inner.write_batch(batch)
    }

    fn flush(&self) -> StorageResult<()> {
        Ok(())
    }

    fn compact(&self) -> StorageResult<()> {
        Ok(())
    }

    fn set_byte_limit(&self, _limit: Option<usize>) -> StorageResult<()> {
        Ok(())
    }

    fn metrics(&self) -> StorageResult<StorageMetrics> {
        Ok(StorageMetrics {
            backend: match self.backend {
                StorageBackendChoice::LegacyRocksDb => "rocksdb-compat",
                StorageBackendChoice::Inhouse => "inhouse",
                StorageBackendChoice::Memory => "memory",
            },
            ..StorageMetrics::default()
        })
    }
}

#[derive(Default)]
struct InMemoryStore {
    data: Arc<Mutex<HashMap<(String, Vec<u8>), Vec<u8>>>>,
}

impl InMemoryStore {
    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        let mut guard = self.data.lock().unwrap();
        guard
            .entry((cf.to_string(), Vec::new()))
            .or_insert_with(Vec::new);
        Ok(())
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let guard = self.data.lock().unwrap();
        Ok(guard.get(&(cf.to_string(), key.to_vec())).cloned())
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let mut guard = self.data.lock().unwrap();
        Ok(guard.insert((cf.to_string(), key.to_vec()), value.to_vec()))
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.put(cf, key, value)?;
        Ok(())
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let mut guard = self.data.lock().unwrap();
        Ok(guard.remove(&(cf.to_string(), key.to_vec())))
    }

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<MemoryIter> {
        let guard = self.data.lock().unwrap();
        let mut entries = Vec::new();
        for ((column, key), value) in guard.iter() {
            if column == cf && key.starts_with(prefix) {
                entries.push((key.clone(), value.clone()));
            }
        }
        Ok(MemoryIter { entries, index: 0 })
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        let guard = self.data.lock().unwrap();
        let mut seen = HashSet::new();
        for key in guard.keys() {
            if !key.0.is_empty() {
                seen.insert(key.0.clone());
            }
        }
        Ok(seen.into_iter().collect())
    }

    fn write_batch(&self, batch: MemoryBatch) -> StorageResult<()> {
        let mut guard = self.data.lock().unwrap();
        for op in batch.ops {
            match op {
                BatchOp::Put(cf, key, value) => {
                    guard.insert((cf, key), value);
                }
                BatchOp::Delete(cf, key) => {
                    guard.remove(&(cf, key));
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct MemoryBatch {
    ops: Vec<BatchOp>,
}

impl KeyValueBatch for MemoryBatch {
    fn put(&mut self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.ops
            .push(BatchOp::Put(cf.to_string(), key.to_vec(), value.to_vec()));
        Ok(())
    }

    fn delete(&mut self, cf: &str, key: &[u8]) -> StorageResult<()> {
        self.ops.push(BatchOp::Delete(cf.to_string(), key.to_vec()));
        Ok(())
    }
}

enum BatchOp {
    Put(String, Vec<u8>, Vec<u8>),
    Delete(String, Vec<u8>),
}

struct MemoryIter {
    entries: Vec<(Vec<u8>, Vec<u8>)>,
    index: usize,
}

impl KeyValueIterator for MemoryIter {
    fn next(&mut self) -> StorageResult<Option<(Vec<u8>, Vec<u8>)>> {
        if self.index >= self.entries.len() {
            Ok(None)
        } else {
            let entry = self.entries[self.index].clone();
            self.index += 1;
            Ok(Some(entry))
        }
    }
}

impl RuntimeBackendChoice {
    pub const fn variants() -> &'static [&'static str] {
        &["inhouse", "stub"]
    }

    fn as_env(&self) -> &'static str {
        match self {
            RuntimeBackendChoice::Inhouse => "inhouse",
            RuntimeBackendChoice::Stub => "stub",
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.as_env()
    }
}

impl TransportBackendChoice {
    pub const fn variants() -> &'static [&'static str] {
        &["quinn", "s2n"]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TransportBackendChoice::Quinn => "quinn",
            TransportBackendChoice::S2n => "s2n",
        }
    }
}

impl OverlayBackendChoice {
    pub const fn variants() -> &'static [&'static str] {
        &["inhouse", "stub"]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            OverlayBackendChoice::Inhouse => "inhouse",
            OverlayBackendChoice::Stub => "stub",
        }
    }
}

impl StorageBackendChoice {
    pub const fn variants() -> &'static [&'static str] {
        &["rocksdb-compat", "inhouse", "memory"]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            StorageBackendChoice::LegacyRocksDb => "rocksdb-compat",
            StorageBackendChoice::Inhouse => "inhouse",
            StorageBackendChoice::Memory => "memory",
        }
    }
}

impl CodingBackendChoice {
    pub const fn variants() -> &'static [&'static str] {
        &["reed-solomon", "xor"]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CodingBackendChoice::ReedSolomon => "reed-solomon",
            CodingBackendChoice::Xor => "xor",
        }
    }
}

impl CryptoBackendChoice {
    pub const fn variants() -> &'static [&'static str] {
        &["dalek", "fallback"]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CryptoBackendChoice::Dalek => "dalek",
            CryptoBackendChoice::Fallback => "fallback",
        }
    }
}

impl CodecBackendChoice {
    pub const fn variants() -> &'static [&'static str] {
        &["json", "binary"]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CodecBackendChoice::Json => "json",
            CodecBackendChoice::Binary => "binary",
        }
    }
}

impl std::str::FromStr for RuntimeBackendChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "inhouse" => Ok(Self::Inhouse),
            "stub" => Ok(Self::Stub),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for TransportBackendChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "quinn" => Ok(Self::Quinn),
            "s2n" => Ok(Self::S2n),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for OverlayBackendChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "inhouse" | "libp2p" => Ok(Self::Inhouse),
            "stub" => Ok(Self::Stub),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for StorageBackendChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "rocksdb-compat" | "rocksdb" => Ok(Self::LegacyRocksDb),
            "inhouse" => Ok(Self::Inhouse),
            "memory" => Ok(Self::Memory),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for CodingBackendChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "reed-solomon" | "reedsolomon" | "reed_s" => Ok(Self::ReedSolomon),
            "xor" => Ok(Self::Xor),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for CryptoBackendChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "dalek" => Ok(Self::Dalek),
            "fallback" => Ok(Self::Fallback),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for CodecBackendChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "binary" => Ok(Self::Binary),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for FaultTarget {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "runtime" => Ok(Self::Runtime),
            "transport" => Ok(Self::Transport),
            "overlay" => Ok(Self::Overlay),
            "storage" => Ok(Self::Storage),
            "coding" => Ok(Self::Coding),
            "crypto" => Ok(Self::Crypto),
            "codec" => Ok(Self::Codec),
            other => Err(other.to_string()),
        }
    }
}

impl std::str::FromStr for FaultKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "timeout" => Ok(Self::Timeout),
            "panic" => Ok(Self::Panic),
            other => Err(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fault_spec() {
        let spec: FaultSpec = "runtime:timeout".parse().expect("parse");
        assert_eq!(spec.target, FaultTarget::Runtime);
        assert_eq!(spec.kind, FaultKind::Timeout);
    }
}

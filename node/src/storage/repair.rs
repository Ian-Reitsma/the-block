use super::erasure::{self, ErasureParams};
use super::types::{ObjectManifest, Redundancy};
use crate::simple_db::{names, SimpleDb};
use crate::storage::settings;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    STORAGE_REPAIR_ATTEMPTS_TOTAL, STORAGE_REPAIR_BYTES_TOTAL, STORAGE_REPAIR_FAILURES_TOTAL,
};
use crypto_suite::hashing::blake3::Hasher;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;
const MAX_CONCURRENT_REPAIRS: usize = 4;
const MAX_LOG_FILES: usize = 14;
const FAILURE_PREFIX: &str = "repair/failures/";
const FAILURE_BACKOFF_BASE_SECS: u64 = 30;
const FAILURE_BACKOFF_CAP_SECS: u64 = 60 * 60;

static REPAIR_POOL: Lazy<ThreadPool> = Lazy::new(|| {
    ThreadPoolBuilder::new()
        .num_threads(MAX_CONCURRENT_REPAIRS)
        .thread_name(|idx| format!("repair-worker-{idx}"))
        .build()
        .expect("repair pool")
});

pub fn spawn(path: String, period: Duration) {
    let _ = runtime::spawn_blocking(move || {
        let mut db = SimpleDb::open_named(names::STORAGE_REPAIR, &path);
        let log = RepairLog::new(Path::new(&path).join("repair_log"));
        loop {
            if let Err(err) = run_once(&mut db, &log, RepairRequest::default()) {
                #[cfg(not(feature = "telemetry"))]
                let _ = &err;
                #[cfg(feature = "telemetry")]
                {
                    let algorithms = settings::algorithms();
                    STORAGE_REPAIR_FAILURES_TOTAL
                        .with_label_values(&[
                            err.label(),
                            algorithms.erasure(),
                            algorithms.compression(),
                        ])
                        .inc();
                    STORAGE_REPAIR_ATTEMPTS_TOTAL
                        .with_label_values(&["fatal"])
                        .inc();
                }
            }
            notify_iteration();
            if should_stop() {
                break;
            }
            thread::sleep(period);
        }
    });
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RepairRequest {
    pub manifest: Option<[u8; 32]>,
    pub chunk: Option<usize>,
    pub force: bool,
}

fn manifest_algorithms(db: &SimpleDb, manifest_hex: &str) -> (String, String) {
    let defaults = settings::algorithms();
    let key = format!("manifest/{manifest_hex}");
    if let Some(bytes) = db.get(&key) {
        if let Ok(manifest) = bincode::deserialize::<ObjectManifest>(&bytes) {
            let erasure = manifest
                .erasure_alg
                .clone()
                .unwrap_or_else(|| defaults.erasure().to_string());
            let compression = manifest
                .compression_alg
                .clone()
                .unwrap_or_else(|| defaults.compression().to_string());
            return (erasure, compression);
        }
    }
    (
        defaults.erasure().to_string(),
        defaults.compression().to_string(),
    )
}

fn manifest_erasure_params(manifest: &ObjectManifest) -> Result<ErasureParams, String> {
    match manifest.redundancy {
        Redundancy::ReedSolomon { data, parity } => {
            let algorithm = manifest
                .erasure_alg
                .clone()
                .unwrap_or_else(|| settings::algorithms().erasure().to_string());
            Ok(ErasureParams::new(
                algorithm,
                data as usize,
                parity as usize,
            ))
        }
        Redundancy::None => Err("manifest has no erasure redundancy".into()),
    }
}

#[derive(Clone, Debug, Default)]
pub struct RepairSummary {
    pub manifests: usize,
    pub attempts: usize,
    pub successes: usize,
    pub failures: usize,
    pub skipped: usize,
    pub bytes_repaired: u64,
    pub failure_details: Vec<RepairFailure>,
}

#[derive(Clone, Debug)]
pub struct RepairFailure {
    pub manifest: String,
    pub chunk: Option<usize>,
    pub error: RepairErrorKind,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairLogStatus {
    Success,
    Failure,
    Skipped,
    Fatal,
}

impl fmt::Display for RepairLogStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            RepairLogStatus::Success => "success",
            RepairLogStatus::Failure => "failure",
            RepairLogStatus::Skipped => "skipped",
            RepairLogStatus::Fatal => "fatal",
        };
        f.write_str(label)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepairLogEntry {
    pub timestamp: i64,
    pub manifest: String,
    pub chunk: Option<u32>,
    pub status: RepairLogStatus,
    pub bytes: u64,
    pub missing_slots: Vec<u32>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RepairLog {
    dir: PathBuf,
}

impl RepairLog {
    pub fn new<P: Into<PathBuf>>(dir: P) -> Self {
        let dir = dir.into();
        if let Err(err) = fs::create_dir_all(&dir) {
            if err.kind() != io::ErrorKind::AlreadyExists {
                panic!("repair log dir: {err}");
            }
        }
        Self { dir }
    }

    pub fn append(&self, entry: &RepairLogEntry) -> Result<(), io::Error> {
        fs::create_dir_all(&self.dir)?;
        let path = self.current_file_path();
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let line =
            serde_json::to_vec(entry).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        file.write_all(&line)?;
        file.write_all(b"\n")?;
        self.prune_old_files()?;
        Ok(())
    }

    pub fn recent_entries(&self, limit: usize) -> Result<Vec<RepairLogEntry>, io::Error> {
        let mut files: Vec<_> = fs::read_dir(&self.dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        files.sort();
        files.reverse();
        let mut entries = Vec::new();
        for file in files {
            let fh = OpenOptions::new().read(true).open(&file)?;
            let reader = BufReader::new(fh);
            for line in reader.lines().flatten() {
                if let Ok(entry) = serde_json::from_slice::<RepairLogEntry>(line.as_bytes()) {
                    entries.push(entry);
                    if entries.len() >= limit {
                        break;
                    }
                }
            }
            if entries.len() >= limit {
                break;
            }
        }
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        entries.truncate(limit);
        Ok(entries)
    }

    fn current_file_path(&self) -> PathBuf {
        let stamp = OffsetDateTime::now_utc()
            .format(&Iso8601::DEFAULT)
            .unwrap_or_else(|_| "unknown".into())
            .replace(':', "-");
        self.dir.join(format!("repair-{stamp}.jsonl"))
    }

    fn prune_old_files(&self) -> Result<(), io::Error> {
        let mut files: Vec<_> = fs::read_dir(&self.dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        if files.len() <= MAX_LOG_FILES {
            return Ok(());
        }
        files.sort();
        while files.len() > MAX_LOG_FILES {
            if let Some(path) = files.first().cloned() {
                let _ = fs::remove_file(&path);
            }
            files.remove(0);
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

#[derive(Clone, Debug)]
pub enum RepairErrorKind {
    Manifest,
    Integrity,
    Reconstruction,
    Encode,
    Database,
    Backoff,
    Fatal,
}

impl RepairErrorKind {
    fn label(&self) -> &'static str {
        match self {
            RepairErrorKind::Manifest => "manifest",
            RepairErrorKind::Integrity => "integrity",
            RepairErrorKind::Reconstruction => "reconstruct",
            RepairErrorKind::Encode => "encode",
            RepairErrorKind::Database => "database",
            RepairErrorKind::Backoff => "backoff",
            RepairErrorKind::Fatal => "fatal",
        }
    }
}

#[derive(Clone, Debug)]
pub enum SkipReason {
    Backoff {
        next_retry_at: i64,
    },
    AlgorithmLimited {
        algorithm: String,
        missing: usize,
        parity_available: usize,
    },
}

#[derive(Clone, Debug)]
enum RepairOutcome {
    Success {
        manifest: String,
        chunk: usize,
        bytes: u64,
        writes: Vec<ShardWrite>,
        missing_slots: Vec<usize>,
        failure_key: String,
    },
    Failure {
        manifest: String,
        chunk: Option<usize>,
        failure_key: Option<String>,
        error: RepairErrorKind,
        message: String,
    },
    Skipped {
        manifest: String,
        chunk: usize,
        reason: SkipReason,
    },
}

#[derive(Clone, Debug)]
struct ShardWrite {
    key: String,
    value: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FailureRecord {
    attempts: u32,
    next_retry_at: i64,
}

impl Default for FailureRecord {
    fn default() -> Self {
        Self {
            attempts: 0,
            next_retry_at: 0,
        }
    }
}

#[derive(Debug)]
pub enum RepairFatalError {
    Log(io::Error),
}

impl RepairFatalError {
    pub fn label(&self) -> &'static str {
        match self {
            RepairFatalError::Log(_) => "log",
        }
    }
}

pub fn run_once(
    db: &mut SimpleDb,
    log: &RepairLog,
    request: RepairRequest,
) -> Result<RepairSummary, RepairFatalError> {
    let mut summary = RepairSummary::default();
    let keys = db.keys_with_prefix("manifest/");
    for key in keys {
        if let Some(filter) = request.manifest {
            let hex = key.trim_start_matches("manifest/");
            if let Ok(bytes) = hex::decode(hex) {
                if bytes != filter {
                    continue;
                }
            } else {
                continue;
            }
        }
        summary.manifests += 1;
        let bytes = match db.get(&key) {
            Some(bytes) => bytes,
            None => {
                summary.failures += 1;
                let entry = RepairLogEntry {
                    timestamp: current_timestamp(),
                    manifest: key.trim_start_matches("manifest/").to_string(),
                    chunk: None,
                    status: RepairLogStatus::Failure,
                    bytes: 0,
                    missing_slots: Vec::new(),
                    error: Some("missing manifest".into()),
                };
                log.append(&entry).map_err(RepairFatalError::Log)?;
                continue;
            }
        };
        let manifest: ObjectManifest = match bincode::deserialize(&bytes) {
            Ok(m) => m,
            Err(err) => {
                summary.failures += 1;
                let entry = RepairLogEntry {
                    timestamp: current_timestamp(),
                    manifest: key.trim_start_matches("manifest/").to_string(),
                    chunk: None,
                    status: RepairLogStatus::Failure,
                    bytes: 0,
                    missing_slots: Vec::new(),
                    error: Some(format!("manifest decode: {err}")),
                };
                log.append(&entry).map_err(RepairFatalError::Log)?;
                continue;
            }
        };

        if let Err(reason) = validate_manifest(&manifest) {
            summary.failures += 1;
            let manifest_hex = key.trim_start_matches("manifest/").to_string();
            summary.failure_details.push(RepairFailure {
                manifest: manifest_hex.clone(),
                chunk: None,
                error: RepairErrorKind::Manifest,
                message: reason.clone(),
            });
            log.append(&RepairLogEntry {
                timestamp: current_timestamp(),
                manifest: manifest_hex,
                chunk: None,
                status: RepairLogStatus::Failure,
                bytes: 0,
                missing_slots: Vec::new(),
                error: Some(reason),
            })
            .map_err(RepairFatalError::Log)?;
            continue;
        }

        if let Redundancy::ReedSolomon { .. } = manifest.redundancy {
            let manifest_hex = key.trim_start_matches("manifest/").to_string();
            let mut jobs = Vec::new();
            let mut outcomes = Vec::new();
            let params = match manifest_erasure_params(&manifest) {
                Ok(p) => p,
                Err(reason) => {
                    outcomes.push(RepairOutcome::Failure {
                        manifest: manifest_hex.clone(),
                        chunk: None,
                        failure_key: None,
                        error: RepairErrorKind::Manifest,
                        message: reason,
                    });
                    for outcome in outcomes {
                        handle_outcome(db, log, &mut summary, outcome)?;
                    }
                    continue;
                }
            };
            let shards_per_chunk = erasure::total_shards_for_params(&params);
            for (chunk_idx, group) in manifest.chunks.chunks(shards_per_chunk).enumerate() {
                if let Some(filter_chunk) = request.chunk {
                    if chunk_idx != filter_chunk {
                        continue;
                    }
                }
                let failure_key = failure_key(&manifest_hex, chunk_idx);
                let now = current_timestamp();
                if !request.force {
                    if let Some(record) = load_failure_record(db, &failure_key) {
                        if now < record.next_retry_at {
                            outcomes.push(RepairOutcome::Skipped {
                                manifest: manifest_hex.clone(),
                                chunk: chunk_idx,
                                reason: SkipReason::Backoff {
                                    next_retry_at: record.next_retry_at,
                                },
                            });
                            continue;
                        }
                    }
                }

                let mut shards = vec![None; shards_per_chunk];
                let mut missing = Vec::new();
                let mut integrity_error = None;
                for (slot, ch) in group.iter().enumerate() {
                    let chunk_key = format!("chunk/{}", hex::encode(ch.id));
                    let blob = db.get(&chunk_key);
                    if let Some(ref data) = blob {
                        let computed = compute_shard_id(slot, data);
                        if computed != ch.id {
                            integrity_error = Some(format!("shard hash mismatch at slot {slot}"));
                            break;
                        }
                    } else {
                        missing.push(slot);
                    }
                    shards[slot] = blob;
                }
                if let Some(reason) = integrity_error {
                    outcomes.push(RepairOutcome::Failure {
                        manifest: manifest_hex.clone(),
                        chunk: Some(chunk_idx),
                        failure_key: Some(failure_key.clone()),
                        error: RepairErrorKind::Integrity,
                        message: reason,
                    });
                    continue;
                }

                let missing_data = missing
                    .iter()
                    .filter(|&&slot| slot < params.data_shards)
                    .count();
                let available_parity = (params.data_shards..params.total_rs())
                    .filter(|&slot| shards.get(slot).and_then(|s| s.as_ref()).is_some())
                    .count();

                if params.is_xor() && missing_data > 0 {
                    let insufficient = missing_data > 1 || available_parity == 0;
                    if insufficient {
                        outcomes.push(RepairOutcome::Skipped {
                            manifest: manifest_hex.clone(),
                            chunk: chunk_idx,
                            reason: SkipReason::AlgorithmLimited {
                                algorithm: params.algorithm.clone(),
                                missing: missing_data,
                                parity_available: available_parity,
                            },
                        });
                        continue;
                    }
                }

                if missing.is_empty() && !request.force {
                    continue;
                }

                let expected_cipher = manifest.chunk_cipher_len(chunk_idx);
                if expected_cipher == 0 {
                    outcomes.push(RepairOutcome::Failure {
                        manifest: manifest_hex.clone(),
                        chunk: Some(chunk_idx),
                        failure_key: Some(failure_key.clone()),
                        error: RepairErrorKind::Manifest,
                        message: "zero-length chunk".into(),
                    });
                    continue;
                }

                let missing_slots = if request.force && missing.is_empty() {
                    (0..shards_per_chunk).collect()
                } else {
                    missing.clone()
                };

                jobs.push(ScheduledJob {
                    manifest: manifest_hex.clone(),
                    chunk: chunk_idx,
                    shards,
                    entries: group.to_vec(),
                    expected_cipher,
                    missing_slots,
                    failure_key: failure_key.clone(),
                    erasure: params.clone(),
                });
            }

            let computed: Vec<RepairOutcome> = if jobs.is_empty() {
                Vec::new()
            } else {
                REPAIR_POOL.install(|| jobs.into_par_iter().map(process_job).collect())
            };

            outcomes.extend(computed);

            for outcome in outcomes {
                handle_outcome(db, log, &mut summary, outcome)?;
            }
        }
    }
    Ok(summary)
}

#[derive(Clone, Debug)]
struct ScheduledJob {
    manifest: String,
    chunk: usize,
    shards: Vec<Option<Vec<u8>>>,
    entries: Vec<super::types::ChunkRef>,
    expected_cipher: usize,
    missing_slots: Vec<usize>,
    failure_key: String,
    erasure: ErasureParams,
}

fn process_job(job: ScheduledJob) -> RepairOutcome {
    match erasure::reconstruct_with_params(job.shards.clone(), job.expected_cipher, &job.erasure) {
        Ok(rebuilt) => match erasure::encode_with_params(&rebuilt, &job.erasure) {
            Ok(encoded) => {
                if encoded.len() != job.entries.len() {
                    return RepairOutcome::Failure {
                        manifest: job.manifest,
                        chunk: Some(job.chunk),
                        failure_key: Some(job.failure_key),
                        error: RepairErrorKind::Encode,
                        message: "encoded shard count mismatch".into(),
                    };
                }
                let mut writes = Vec::new();
                let mut bytes = 0u64;
                for &idx in &job.missing_slots {
                    if let Some(shard) = encoded.get(idx) {
                        let shard_id = job.entries[idx].id;
                        if compute_shard_id(idx, shard) != shard_id {
                            return RepairOutcome::Failure {
                                manifest: job.manifest,
                                chunk: Some(job.chunk),
                                failure_key: Some(job.failure_key),
                                error: RepairErrorKind::Integrity,
                                message: format!("re-encoded shard hash mismatch at slot {idx}"),
                            };
                        }
                        writes.push(ShardWrite {
                            key: format!("chunk/{}", hex::encode(shard_id)),
                            value: shard.clone(),
                        });
                        bytes = bytes.saturating_add(shard.len() as u64);
                    } else {
                        return RepairOutcome::Failure {
                            manifest: job.manifest,
                            chunk: Some(job.chunk),
                            failure_key: Some(job.failure_key),
                            error: RepairErrorKind::Encode,
                            message: format!("missing encoded shard at slot {idx}"),
                        };
                    }
                }
                RepairOutcome::Success {
                    manifest: job.manifest,
                    chunk: job.chunk,
                    bytes,
                    writes,
                    missing_slots: job.missing_slots,
                    failure_key: job.failure_key,
                }
            }
            Err(err) => RepairOutcome::Failure {
                manifest: job.manifest,
                chunk: Some(job.chunk),
                failure_key: Some(job.failure_key),
                error: RepairErrorKind::Encode,
                message: err,
            },
        },
        Err(err) => RepairOutcome::Failure {
            manifest: job.manifest,
            chunk: Some(job.chunk),
            failure_key: Some(job.failure_key),
            error: RepairErrorKind::Reconstruction,
            message: err,
        },
    }
}

fn handle_outcome(
    db: &mut SimpleDb,
    log: &RepairLog,
    summary: &mut RepairSummary,
    outcome: RepairOutcome,
) -> Result<(), RepairFatalError> {
    let timestamp = current_timestamp();
    match outcome {
        RepairOutcome::Success {
            manifest,
            chunk,
            bytes,
            writes,
            missing_slots,
            failure_key,
        } => {
            let mut db_error = None;
            for write in &writes {
                if let Err(err) = db.try_insert(&write.key, write.value.clone()) {
                    db_error = Some(err.to_string());
                    break;
                }
            }
            if let Some(err) = db_error {
                update_failure_record(db, &failure_key, false);
                summary.failures += 1;
                summary.failure_details.push(RepairFailure {
                    manifest: manifest.clone(),
                    chunk: Some(chunk),
                    error: RepairErrorKind::Database,
                    message: err.clone(),
                });
                #[cfg(feature = "telemetry")]
                {
                    let (erasure_alg, compression_alg) = manifest_algorithms(db, &manifest);
                    STORAGE_REPAIR_ATTEMPTS_TOTAL
                        .with_label_values(&["failure"])
                        .inc();
                    STORAGE_REPAIR_FAILURES_TOTAL
                        .with_label_values(&[
                            RepairErrorKind::Database.label(),
                            erasure_alg.as_str(),
                            compression_alg.as_str(),
                        ])
                        .inc();
                }
                log.append(&RepairLogEntry {
                    timestamp,
                    manifest,
                    chunk: Some(chunk as u32),
                    status: RepairLogStatus::Failure,
                    bytes: 0,
                    missing_slots: missing_slots.iter().map(|s| *s as u32).collect(),
                    error: Some(err),
                })
                .map_err(RepairFatalError::Log)?;
            } else {
                update_failure_record(db, &failure_key, true);
                summary.attempts += 1;
                summary.successes += 1;
                summary.bytes_repaired = summary.bytes_repaired.saturating_add(bytes);
                #[cfg(feature = "telemetry")]
                {
                    STORAGE_REPAIR_ATTEMPTS_TOTAL
                        .with_label_values(&["success"])
                        .inc();
                    if bytes > 0 {
                        STORAGE_REPAIR_BYTES_TOTAL.inc_by(bytes);
                    }
                }
                log.append(&RepairLogEntry {
                    timestamp,
                    manifest,
                    chunk: Some(chunk as u32),
                    status: RepairLogStatus::Success,
                    bytes,
                    missing_slots: missing_slots.iter().map(|s| *s as u32).collect(),
                    error: None,
                })
                .map_err(RepairFatalError::Log)?;
            }
        }
        RepairOutcome::Failure {
            manifest,
            chunk,
            failure_key,
            error,
            message,
        } => {
            if let Some(key) = failure_key {
                update_failure_record(db, &key, false);
            }
            summary.failures += 1;
            summary.attempts += 1;
            summary.failure_details.push(RepairFailure {
                manifest: manifest.clone(),
                chunk,
                error: error.clone(),
                message: message.clone(),
            });
            #[cfg(feature = "telemetry")]
            {
                let (erasure_alg, compression_alg) = manifest_algorithms(db, &manifest);
                STORAGE_REPAIR_ATTEMPTS_TOTAL
                    .with_label_values(&["failure"])
                    .inc();
                STORAGE_REPAIR_FAILURES_TOTAL
                    .with_label_values(&[
                        error.label(),
                        erasure_alg.as_str(),
                        compression_alg.as_str(),
                    ])
                    .inc();
            }
            log.append(&RepairLogEntry {
                timestamp,
                manifest,
                chunk: chunk.map(|c| c as u32),
                status: RepairLogStatus::Failure,
                bytes: 0,
                missing_slots: Vec::new(),
                error: Some(message),
            })
            .map_err(RepairFatalError::Log)?;
        }
        RepairOutcome::Skipped {
            manifest,
            chunk,
            reason,
        } => {
            summary.skipped += 1;
            #[cfg(feature = "telemetry")]
            {
                STORAGE_REPAIR_ATTEMPTS_TOTAL
                    .with_label_values(&["skipped"])
                    .inc();
            }
            match reason {
                SkipReason::Backoff { next_retry_at } => {
                    log.append(&RepairLogEntry {
                        timestamp,
                        manifest,
                        chunk: Some(chunk as u32),
                        status: RepairLogStatus::Skipped,
                        bytes: 0,
                        missing_slots: Vec::new(),
                        error: Some(format!("next_retry_at:{next_retry_at}")),
                    })
                    .map_err(RepairFatalError::Log)?;
                }
                SkipReason::AlgorithmLimited {
                    algorithm,
                    missing,
                    parity_available,
                } => {
                    log.append(&RepairLogEntry {
                        timestamp,
                        manifest,
                        chunk: Some(chunk as u32),
                        status: RepairLogStatus::Skipped,
                        bytes: 0,
                        missing_slots: Vec::new(),
                        error: Some(format!(
                            "algorithm_limited:{algorithm}:missing={missing}:parity={parity_available}"
                        )),
                    })
                    .map_err(RepairFatalError::Log)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_manifest(manifest: &ObjectManifest) -> Result<(), String> {
    if let Redundancy::ReedSolomon { .. } = manifest.redundancy {
        let params = manifest_erasure_params(manifest)?;
        let shards_per_chunk = erasure::total_shards_for_params(&params);
        if shards_per_chunk == 0 {
            return Err("no shards configured".into());
        }
        if manifest.chunks.len() % shards_per_chunk != 0 {
            return Err("chunk list not aligned to shard groups".into());
        }
        let expected_chunks = manifest.chunk_count();
        if expected_chunks * shards_per_chunk != manifest.chunks.len() {
            return Err("manifest chunk count mismatch".into());
        }
    }

    let mut copy = manifest.clone();
    copy.blake3 = [0u8; 32];
    let serialized = bincode::serialize(&copy).map_err(|e| e.to_string())?;
    let mut hasher = Hasher::new();
    hasher.update(&serialized);
    let computed = hasher.finalize();
    if computed.as_bytes() != &manifest.blake3 {
        return Err("manifest hash mismatch".into());
    }
    Ok(())
}

fn load_failure_record(db: &SimpleDb, key: &str) -> Option<FailureRecord> {
    let store_key = format!("{FAILURE_PREFIX}{key}");
    db.get(&store_key)
        .and_then(|bytes| bincode::deserialize(&bytes).ok())
}

fn update_failure_record(db: &mut SimpleDb, key: &str, success: bool) {
    let store_key = format!("{FAILURE_PREFIX}{key}");
    if success {
        let _ = db.remove(&store_key);
        return;
    }
    let mut record = load_failure_record(db, key).unwrap_or_default();
    record.attempts = record.attempts.saturating_add(1);
    let exponent = record.attempts.saturating_sub(1).min(31);
    let multiplier = 1u64 << exponent;
    let backoff = FAILURE_BACKOFF_BASE_SECS.saturating_mul(multiplier);
    let capped = backoff.min(FAILURE_BACKOFF_CAP_SECS);
    record.next_retry_at = current_timestamp().saturating_add(capped as i64);
    if let Ok(bytes) = bincode::serialize(&record) {
        let _ = db.try_insert(&store_key, bytes);
    }
}

fn failure_key(manifest: &str, chunk_idx: usize) -> String {
    format!("{manifest}:{chunk_idx}")
}

fn compute_shard_id(slot: usize, shard: &[u8]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(&[slot as u8]);
    h.update(shard);
    let mut out = [0u8; 32];
    out.copy_from_slice(h.finalize().as_bytes());
    out
}

fn current_timestamp() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Encodes `data` into fountain packets with the BLE-tuned parameters and decodes
/// them after dropping a single packet, returning the recovered bytes.
pub fn fountain_repair_roundtrip(data: &[u8]) -> Result<Vec<u8>, String> {
    let coder = settings::fountain();
    let batch = coder.encode(data).map_err(|e| e.to_string())?;
    let (metadata, mut packets) = batch.into_parts();
    if !packets.is_empty() {
        packets.remove(0);
    }
    coder.decode(&metadata, &packets).map_err(|e| e.to_string())
}

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use tokio::sync::mpsc::UnboundedSender;

#[cfg(test)]
static ITERATION_HOOK: Lazy<Mutex<Option<UnboundedSender<()>>>> = Lazy::new(|| Mutex::new(None));
#[cfg(test)]
static STOP_FLAG: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

#[cfg(test)]
pub(crate) fn install_iteration_hook(sender: UnboundedSender<()>) {
    *ITERATION_HOOK.lock().unwrap() = Some(sender);
}

#[cfg(test)]
pub(crate) fn clear_iteration_hook() {
    ITERATION_HOOK.lock().unwrap().take();
    STOP_FLAG.store(false, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn request_stop() {
    STOP_FLAG.store(true, Ordering::SeqCst);
}

#[cfg(test)]
fn notify_iteration() {
    if let Some(tx) = ITERATION_HOOK.lock().unwrap().as_ref() {
        let _ = tx.send(());
    }
}

#[cfg(not(test))]
fn notify_iteration() {}

#[cfg(test)]
fn should_stop() -> bool {
    STOP_FLAG.load(Ordering::SeqCst)
}

#[cfg(not(test))]
fn should_stop() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn spawn_runs_loop_and_signals_iterations() {
        let tempdir = tempdir().expect("temp dir");
        let path = tempdir.path().join("repair-db");
        let path_str = path.to_str().expect("path").to_string();
        runtime::block_on(async move {
            let _guard = tempdir; // keep directory alive for the background task
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            install_iteration_hook(tx);
            spawn(path_str, Duration::from_millis(10));

            for _ in 0..2 {
                runtime::timeout(Duration::from_secs(1), rx.recv())
                    .await
                    .expect("timer")
                    .expect("iteration");
            }

            request_stop();
            runtime::sleep(Duration::from_millis(20)).await;
            clear_iteration_hook();
        });
    }
}

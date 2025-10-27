#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3;
use crypto_suite::hex;
use crypto_suite::signatures::ed25519::{SigningKey, SECRET_KEY_LENGTH};
use foundation_object_store::{S3Client, UploadError, UploadReceipt};
use foundation_serialization::json::{self, Map, Number, Value};
use foundation_time::UtcDateTime;
use http_env::blocking_client as env_blocking_client;
use httpd::{BlockingClient, Method};
use monitoring_build::{
    sign_attestation, ChaosAttestation, ChaosReadinessSnapshot, ChaosSnapshotDecodeError,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use sys::archive::zip::ZipBuilder;
use tb_sim::chaos::{ChaosModule, ChaosProviderKind, ChaosSite};
use tb_sim::Simulation;

const STATUS_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

fn signing_key_from_env() -> Result<SigningKey, Box<dyn Error>> {
    match env::var("TB_CHAOS_SIGNING_KEY") {
        Ok(hex_key) => {
            let key_bytes = hex::decode_array::<SECRET_KEY_LENGTH>(&hex_key)
                .map_err(|_| "TB_CHAOS_SIGNING_KEY must be a valid hex-encoded ed25519 secret")?;
            Ok(SigningKey::from_bytes(&key_bytes))
        }
        Err(_) => {
            use rand::rngs::OsRng;
            eprintln!("[chaos-lab] TB_CHAOS_SIGNING_KEY missing; generating ephemeral signing key");
            let mut rng = OsRng::default();
            Ok(SigningKey::generate(&mut rng))
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let steps = env::var("TB_CHAOS_STEPS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120);
    let nodes = env::var("TB_CHAOS_NODE_COUNT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(256);
    let dashboard_path = env::var("TB_CHAOS_DASHBOARD").ok();
    let attestation_path =
        env::var("TB_CHAOS_ATTESTATIONS").unwrap_or_else(|_| "chaos_attestations.json".to_string());
    let status_snapshot_path = env::var("TB_CHAOS_STATUS_SNAPSHOT")
        .ok()
        .filter(|value| !value.is_empty());
    let diff_path_env = env::var("TB_CHAOS_STATUS_DIFF")
        .ok()
        .filter(|value| !value.is_empty());
    let baseline_path_env = env::var("TB_CHAOS_STATUS_BASELINE")
        .ok()
        .filter(|value| !value.is_empty());
    let overlay_path_env = env::var("TB_CHAOS_OVERLAY_READINESS")
        .ok()
        .filter(|value| !value.is_empty());
    let provider_failover_env = env::var("TB_CHAOS_PROVIDER_FAILOVER")
        .ok()
        .filter(|value| !value.is_empty());
    let status_endpoint = env::var("TB_CHAOS_STATUS_ENDPOINT")
        .ok()
        .filter(|value| !value.is_empty());
    let require_diff = env::var("TB_CHAOS_REQUIRE_DIFF")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false);
    let signing_key = signing_key_from_env()?;

    let baseline_from_endpoint = if let Some(ref endpoint) = status_endpoint {
        let baseline = fetch_status_snapshot(endpoint)?;
        if let Some(ref path) = baseline_path_env {
            persist_status_snapshot(path, &baseline)?;
            eprintln!("[chaos-lab] fetched chaos/status baseline from {endpoint} into {path}");
        } else {
            eprintln!("[chaos-lab] fetched chaos/status baseline from {endpoint}");
        }
        Some(baseline)
    } else {
        None
    };

    let baseline_from_file = if baseline_from_endpoint.is_none() {
        match baseline_path_env.as_ref() {
            Some(path) => {
                let baseline = load_status_snapshot(path)?;
                eprintln!("[chaos-lab] loaded chaos/status baseline from {path}");
                Some(baseline)
            }
            None => None,
        }
    } else {
        None
    };

    let mut sim = Simulation::new(nodes);
    apply_site_overrides(&mut sim);
    if let Some(ref path) = dashboard_path {
        sim.run(steps, path)?;
    } else {
        sim.drive(steps);
    }

    let issued_at = UtcDateTime::now().unix_timestamp().unwrap_or_default() as u64;
    let drafts = sim.chaos_attestation_drafts(issued_at);
    let attestations: Vec<ChaosAttestation> = drafts
        .into_iter()
        .map(|draft| sign_attestation(draft, &signing_key))
        .collect();

    let snapshots: Vec<ChaosReadinessSnapshot> = attestations
        .iter()
        .map(ChaosReadinessSnapshot::from)
        .collect();

    if let Some(ref path) = status_snapshot_path {
        persist_status_snapshot(path, &snapshots)?;
        eprintln!("[chaos-lab] wrote chaos status snapshot to {path}");
    }

    let baseline_for_diff = baseline_from_endpoint
        .as_ref()
        .map(|entries| entries.as_slice())
        .or_else(|| {
            baseline_from_file
                .as_ref()
                .map(|entries| entries.as_slice())
        });

    if let Some(ref path) = overlay_path_env {
        let total = persist_overlay_readiness(path, &snapshots, baseline_for_diff)?;
        eprintln!("[chaos-lab] wrote {total} overlay readiness entries to {path}");
    }

    let diff_path = diff_path_env
        .clone()
        .unwrap_or_else(|| "chaos_status_diff.json".to_string());

    if let Some(baseline) = baseline_for_diff {
        let diffs = compute_status_diff(baseline, &snapshots);
        persist_status_diff(&diff_path, &diffs)?;
        eprintln!(
            "[chaos-lab] chaos status diff entries={} path={}",
            diffs.len(),
            diff_path
        );
        if require_diff && diffs.is_empty() {
            return Err("expected chaos/status diff but none detected".into());
        }
    } else {
        let empty: Vec<StatusDiffEntry> = Vec::new();
        persist_status_diff(&diff_path, &empty)?;
        eprintln!(
            "[chaos-lab] no baseline provided; wrote empty chaos/status diff to {}",
            diff_path
        );
        if require_diff {
            return Err("TB_CHAOS_REQUIRE_DIFF set but no baseline was available".into());
        }
    }

    let provider_failover_path = provider_failover_env
        .clone()
        .unwrap_or_else(|| "chaos_provider_failover.json".to_string());
    let provider_outcome =
        provider_failover_reports(baseline_for_diff.unwrap_or(&snapshots), &snapshots);
    persist_provider_failover_reports(&provider_failover_path, &provider_outcome.reports)?;
    for report in &provider_outcome.reports {
        if report.scenarios.is_empty() {
            eprintln!(
                "[chaos-lab] provider failover provider={} skipped (no overlay sites)",
                report.provider
            );
        } else {
            eprintln!(
                "[chaos-lab] provider failover provider={} scenarios={} diff_entries={}",
                report.provider,
                report.scenarios.len(),
                report.total_diff_entries
            );
        }
    }
    if !provider_outcome.failures.is_empty() {
        return Err(provider_outcome.failures.join("; ").into());
    }

    persist_attestations(&attestation_path, &attestations)?;
    eprintln!(
        "[chaos-lab] captured {} attestations for modules: {}",
        attestations.len(),
        format_modules(&attestations)
    );
    eprintln!(
        "[chaos-lab] verifier={}",
        hex::encode(signing_key.verifying_key().to_bytes())
    );
    let archive_outcome = if let Some(archive_dir) = env::var("TB_CHAOS_ARCHIVE_DIR")
        .ok()
        .filter(|value| !value.is_empty())
    {
        let label = env::var("TB_CHAOS_ARCHIVE_LABEL").ok();
        archive_chaos_run(
            &archive_dir,
            label.as_deref(),
            &snapshots,
            &attestation_path,
            status_snapshot_path.as_deref(),
            &diff_path,
            overlay_path_env.as_deref(),
            &provider_failover_path,
        )?
    } else {
        None
    };
    if let Some(outcome) = archive_outcome {
        publish_archive_outcome(&outcome)?;
    }
    Ok(())
}

fn persist_attestations(
    path: &str,
    attestations: &[ChaosAttestation],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(attestations.iter().map(|att| att.to_value()).collect());
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

fn archive_chaos_run(
    archive_dir: &str,
    label: Option<&str>,
    snapshots: &[ChaosReadinessSnapshot],
    attestation_path: &str,
    snapshot_path: Option<&str>,
    diff_path: &str,
    overlay_path: Option<&str>,
    provider_failover_path: &str,
) -> Result<Option<ChaosArchiveOutcome>, Box<dyn Error>> {
    if snapshots.is_empty() {
        return Ok(None);
    }

    let issued_at = snapshots
        .iter()
        .map(|snapshot| snapshot.issued_at)
        .max()
        .unwrap_or_default();
    let archive_root = Path::new(archive_dir);
    fs::create_dir_all(archive_root)?;

    let mut run_dir = archive_root.join(issued_at.to_string());
    let mut suffix = 0u64;
    while run_dir.exists() {
        suffix = suffix.saturating_add(1);
        run_dir = archive_root.join(format!("{}-{}", issued_at, suffix));
    }
    fs::create_dir_all(&run_dir)?;

    let mut artifacts: BTreeMap<String, ArchivedArtifact> = BTreeMap::new();

    artifacts.insert(
        "attestations".to_string(),
        copy_with_digest(attestation_path, &run_dir)?,
    );

    if let Some(path) = snapshot_path {
        if Path::new(path).exists() {
            artifacts.insert(
                "status_snapshot".to_string(),
                copy_with_digest(path, &run_dir)?,
            );
        }
    }

    if Path::new(diff_path).exists() {
        artifacts.insert(
            "status_diff".to_string(),
            copy_with_digest(diff_path, &run_dir)?,
        );
    }

    if let Some(path) = overlay_path {
        if Path::new(path).exists() {
            artifacts.insert(
                "overlay_readiness".to_string(),
                copy_with_digest(path, &run_dir)?,
            );
        }
    }

    if Path::new(provider_failover_path).exists() {
        artifacts.insert(
            "provider_failover".to_string(),
            copy_with_digest(provider_failover_path, &run_dir)?,
        );
    }

    let run_id = run_dir
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| issued_at.to_string());

    let mut manifest = ChaosArchiveManifest {
        issued_at,
        run_id: run_id.clone(),
        label: label.map(|value| value.to_string()),
        artifacts,
        bundle: None,
    };

    let mut latest = ChaosArchiveLatest {
        issued_at,
        run_id: run_id.clone(),
        label: manifest.label.clone(),
        manifest: format!("{run_id}/manifest.json"),
        bundle: None,
    };

    let manifest_bytes = {
        let value = manifest.to_value();
        json::to_vec_pretty(&value)?
    };
    let latest_bytes = {
        let value = latest.to_value();
        json::to_vec_pretty(&value)?
    };
    let bundle = create_archive_bundle(
        archive_root,
        &run_dir,
        &run_id,
        &manifest_bytes,
        &latest_bytes,
        &manifest.artifacts,
    )?;

    if let Some(bundle) = &bundle {
        manifest.bundle = Some(ArchivedBundle {
            file: bundle.file.clone(),
            blake3: bundle.digest.clone(),
            size: bundle.size,
        });
        latest.bundle = Some(bundle.file.clone());
    }

    let manifest_path = run_dir.join("manifest.json");
    let manifest_value_final = manifest.to_value();
    write_json_value(&manifest_path, &manifest_value_final)?;

    let latest_path = archive_root.join("latest.json");
    let latest_value_final = latest.to_value();
    write_json_value(&latest_path, &latest_value_final)?;

    if let Some(bundle) = &bundle {
        eprintln!(
            "[chaos-lab] archived chaos artefacts to {} (run {}) bundle={} blake3={}",
            run_dir.display(),
            run_id,
            bundle.file,
            bundle.digest
        );
    } else {
        eprintln!(
            "[chaos-lab] archived chaos artefacts to {} (run {})",
            run_dir.display(),
            run_id
        );
    }

    Ok(Some(ChaosArchiveOutcome {
        run_id,
        manifest_path,
        latest_path,
        bundle_path: bundle.as_ref().map(|info| info.path.clone()),
        bundle_file: bundle.as_ref().map(|info| info.file.clone()),
        bundle_digest: bundle.as_ref().map(|info| info.digest.clone()),
    }))
}

fn copy_with_digest(path: &str, dest_dir: &Path) -> Result<ArchivedArtifact, Box<dyn Error>> {
    let source = Path::new(path);
    let name = source
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "artifact.json".to_string());
    let destination = dest_dir.join(&name);
    fs::copy(source, &destination)?;
    let bytes = fs::read(&destination)?;
    let digest = blake3::hash(&bytes).to_hex().to_string();
    Ok(ArchivedArtifact {
        file: name,
        blake3: digest,
    })
}

struct ArchiveBundleResult {
    file: String,
    path: PathBuf,
    digest: String,
    size: u64,
}

fn create_archive_bundle(
    archive_root: &Path,
    run_dir: &Path,
    run_id: &str,
    manifest_bytes: &[u8],
    latest_bytes: &[u8],
    artifacts: &BTreeMap<String, ArchivedArtifact>,
) -> Result<Option<ArchiveBundleResult>, Box<dyn Error>> {
    if artifacts.is_empty() {
        return Ok(None);
    }

    let mut builder = ZipBuilder::new();
    builder.add_file("manifest.json", manifest_bytes)?;
    builder.add_file("latest.json", latest_bytes)?;
    for artifact in artifacts.values() {
        let path = run_dir.join(&artifact.file);
        if path.exists() {
            let bytes = fs::read(&path)?;
            builder.add_file(&artifact.file, &bytes)?;
        }
    }

    let bytes = builder.finish()?;
    let file = format!("{run_id}.zip");
    let path = archive_root.join(&file);
    fs::write(&path, &bytes)?;
    let digest = blake3::hash(&bytes).to_hex().to_string();
    let size = bytes.len() as u64;
    Ok(Some(ArchiveBundleResult {
        file,
        path,
        digest,
        size,
    }))
}

struct ChaosArchiveOutcome {
    run_id: String,
    manifest_path: PathBuf,
    latest_path: PathBuf,
    bundle_path: Option<PathBuf>,
    bundle_file: Option<String>,
    bundle_digest: Option<String>,
}

fn publish_archive_outcome(outcome: &ChaosArchiveOutcome) -> Result<(), Box<dyn Error>> {
    publish_to_directory(outcome)?;
    publish_to_object_store(outcome)?;
    Ok(())
}

fn publish_to_directory(outcome: &ChaosArchiveOutcome) -> Result<(), Box<dyn Error>> {
    let publish_dir = match env::var("TB_CHAOS_ARCHIVE_PUBLISH_DIR")
        .ok()
        .filter(|value| !value.is_empty())
    {
        Some(path) => path,
        None => return Ok(()),
    };
    let publish_root = Path::new(&publish_dir);
    fs::create_dir_all(publish_root)?;

    let latest_dest = publish_root.join("latest.json");
    fs::copy(&outcome.latest_path, &latest_dest)?;

    let run_dest = publish_root.join(&outcome.run_id);
    fs::create_dir_all(&run_dest)?;
    fs::copy(&outcome.manifest_path, run_dest.join("manifest.json"))?;

    if let Some(bundle_path) = &outcome.bundle_path {
        let bundle_name = outcome
            .bundle_file
            .clone()
            .unwrap_or_else(|| format!("{}.zip", outcome.run_id));
        fs::copy(bundle_path, publish_root.join(&bundle_name))?;
        if let Some(digest) = &outcome.bundle_digest {
            eprintln!(
                "[chaos-lab] published bundle {} (blake3={}) to {}",
                bundle_name,
                digest,
                publish_root.display()
            );
        } else {
            eprintln!(
                "[chaos-lab] published bundle {} to {}",
                bundle_name,
                publish_root.display()
            );
        }
    }

    eprintln!(
        "[chaos-lab] mirrored archive run {} to {}",
        outcome.run_id,
        publish_root.display()
    );
    Ok(())
}

struct ArchiveUploadReceipts {
    manifest_key: String,
    manifest: UploadReceipt,
    latest_key: String,
    latest: UploadReceipt,
    bundle: Option<(String, UploadReceipt)>,
}

fn upload_archive_objects(
    client: &S3Client,
    http: &BlockingClient,
    bucket: &str,
    prefix: &str,
    run_id: &str,
    manifest_bytes: &[u8],
    latest_bytes: &[u8],
    bundle_bytes: Option<&[u8]>,
    bundle_file: Option<&str>,
    retry_limit: usize,
    fixed_timestamp: Option<i64>,
) -> Result<ArchiveUploadReceipts, UploadError> {
    upload_archive_objects_with(
        bucket,
        prefix,
        run_id,
        manifest_bytes,
        latest_bytes,
        bundle_bytes,
        bundle_file,
        retry_limit,
        fixed_timestamp,
        |bucket, key, bytes, label, retry, fixed| {
            upload_with_retries(client, http, bucket, key, bytes, label, retry, fixed)
        },
    )
}

fn upload_archive_objects_with<F>(
    bucket: &str,
    prefix: &str,
    run_id: &str,
    manifest_bytes: &[u8],
    latest_bytes: &[u8],
    bundle_bytes: Option<&[u8]>,
    bundle_file: Option<&str>,
    retry_limit: usize,
    fixed_timestamp: Option<i64>,
    mut uploader: F,
) -> Result<ArchiveUploadReceipts, UploadError>
where
    F: FnMut(&str, &str, &[u8], &str, usize, Option<i64>) -> Result<UploadReceipt, UploadError>,
{
    let manifest_key = join_object_key(prefix, &format!("{}/manifest.json", run_id));
    let latest_key = join_object_key(prefix, "latest.json");

    let manifest = uploader(
        bucket,
        &manifest_key,
        manifest_bytes,
        "manifest",
        retry_limit,
        fixed_timestamp,
    )?;

    let latest = uploader(
        bucket,
        &latest_key,
        latest_bytes,
        "latest",
        retry_limit,
        fixed_timestamp,
    )?;

    let bundle = match bundle_bytes {
        Some(bytes) => {
            let file_name = bundle_file
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .unwrap_or_else(|| format!("{run_id}.zip"));
            let key = join_object_key(prefix, &file_name);
            let receipt = uploader(bucket, &key, bytes, "bundle", retry_limit, fixed_timestamp)?;
            Some((key, receipt))
        }
        None => None,
    };

    Ok(ArchiveUploadReceipts {
        manifest_key,
        manifest,
        latest_key,
        latest,
        bundle,
    })
}

fn publish_to_object_store(outcome: &ChaosArchiveOutcome) -> Result<(), Box<dyn Error>> {
    let bucket = match env::var("TB_CHAOS_ARCHIVE_BUCKET")
        .ok()
        .filter(|value| !value.is_empty())
    {
        Some(bucket) => bucket,
        None => return Ok(()),
    };

    let prefix = env::var("TB_CHAOS_ARCHIVE_PREFIX").unwrap_or_else(|_| "chaos".to_string());
    let prefix = prefix.trim_matches('/').to_string();

    let retry_limit = archive_retry_limit()?;
    let fixed_timestamp = archive_fixed_time()?;

    let client = S3Client::from_env()?;
    let http = env_blocking_client(&["TB_CHAOS_ARCHIVE_TLS", "TB_HTTP_TLS"], "chaos-archive");

    let manifest_bytes = fs::read(&outcome.manifest_path)?;
    let latest_bytes = fs::read(&outcome.latest_path)?;
    let bundle_bytes = match &outcome.bundle_path {
        Some(path) => Some(fs::read(path)?),
        None => None,
    };

    let receipts = upload_archive_objects(
        &client,
        &http,
        &bucket,
        &prefix,
        &outcome.run_id,
        &manifest_bytes,
        &latest_bytes,
        bundle_bytes.as_deref(),
        outcome.bundle_file.as_deref(),
        retry_limit,
        fixed_timestamp,
    )
    .map_err(|err| -> Box<dyn Error> { Box::new(err) })?;

    if let Some((bundle_key, bundle_receipt)) = receipts.bundle {
        if let Some(digest) = &outcome.bundle_digest {
            eprintln!(
                "[chaos-lab] uploaded bundle s3://{}/{} (sha256={} blake3={})",
                bucket, bundle_key, bundle_receipt.payload_sha256, digest
            );
        } else {
            eprintln!(
                "[chaos-lab] uploaded bundle s3://{}/{} (sha256={})",
                bucket, bundle_key, bundle_receipt.payload_sha256
            );
        }
    } else {
        eprintln!("[chaos-lab] bundle upload skipped (no bundle generated)");
    }

    eprintln!(
        "[chaos-lab] uploaded manifest s3://{}/{} (sha256={})",
        bucket, receipts.manifest_key, receipts.manifest.payload_sha256
    );
    eprintln!(
        "[chaos-lab] uploaded latest pointer s3://{}/{} (sha256={})",
        bucket, receipts.latest_key, receipts.latest.payload_sha256
    );

    Ok(())
}

fn archive_retry_limit() -> Result<usize, Box<dyn Error>> {
    match env::var("TB_CHAOS_ARCHIVE_RETRIES") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(3);
            }
            let parsed: usize = trimmed
                .parse()
                .map_err(|_| format!("invalid TB_CHAOS_ARCHIVE_RETRIES value: {value}"))?;
            if parsed == 0 {
                Ok(1)
            } else {
                Ok(parsed)
            }
        }
        Err(env::VarError::NotPresent) => Ok(3),
        Err(err) => Err(format!("failed to read TB_CHAOS_ARCHIVE_RETRIES: {err}").into()),
    }
}

fn archive_fixed_time() -> Result<Option<i64>, Box<dyn Error>> {
    match env::var("TB_CHAOS_ARCHIVE_FIXED_TIME") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let timestamp: i64 = trimmed
                .parse()
                .map_err(|_| format!("invalid TB_CHAOS_ARCHIVE_FIXED_TIME value: {value}"))?;
            UtcDateTime::from_unix_timestamp(timestamp)
                .map_err(|_| format!("TB_CHAOS_ARCHIVE_FIXED_TIME out of range: {value}"))?;
            Ok(Some(timestamp))
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(format!("failed to read TB_CHAOS_ARCHIVE_FIXED_TIME: {err}").into()),
    }
}

fn upload_with_retries(
    client: &S3Client,
    http: &BlockingClient,
    bucket: &str,
    key: &str,
    payload: &[u8],
    label: &str,
    retry_limit: usize,
    fixed_timestamp: Option<i64>,
) -> Result<UploadReceipt, UploadError> {
    upload_with_retries_with(
        label,
        retry_limit,
        fixed_timestamp,
        |attempt, total, now| {
            let result = client.put_object_blocking_at(http, bucket, key, payload.to_vec(), now);
            match &result {
                Ok(_) if attempt > 1 => {
                    eprintln!(
                        "[chaos-lab] upload {label} to s3://{bucket}/{key} succeeded after attempt {}",
                        attempt
                    );
                }
                Err(err) => {
                    eprintln!(
                        "[chaos-lab] upload {label} to s3://{bucket}/{key} attempt {}/{} failed: {err}",
                        attempt, total
                    );
                }
                _ => {}
            }
            result
        },
    )
}

fn upload_with_retries_with<F>(
    _label: &str,
    retry_limit: usize,
    fixed_timestamp: Option<i64>,
    mut attempt: F,
) -> Result<UploadReceipt, UploadError>
where
    F: FnMut(usize, usize, UtcDateTime) -> Result<UploadReceipt, UploadError>,
{
    let attempts = retry_limit.max(1);
    for attempt_index in 1..=attempts {
        let now = fixed_timestamp
            .map(|ts| {
                UtcDateTime::from_unix_timestamp(ts)
                    .expect("archive_fixed_time validated timestamp bounds")
            })
            .unwrap_or_else(|| UtcDateTime::from(SystemTime::now()));
        match attempt(attempt_index, attempts, now) {
            Ok(receipt) => return Ok(receipt),
            Err(err) => {
                if attempt_index == attempts {
                    return Err(err);
                }
            }
        }
    }
    unreachable!("retry loop must return before exhaustion");
}

fn join_object_key(prefix: &str, suffix: &str) -> String {
    let trimmed_suffix = suffix.trim_start_matches('/');
    if trimmed_suffix.is_empty() {
        return prefix.trim_matches('/').to_string();
    }
    let trimmed_prefix = prefix.trim_matches('/');
    if trimmed_prefix.is_empty() {
        trimmed_suffix.to_string()
    } else {
        format!("{}/{}", trimmed_prefix, trimmed_suffix)
    }
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let bytes = json::to_vec_pretty(value)?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use sys::tempfile::tempdir;

    struct EnvGuard {
        key: String,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let previous = env::var(key).ok();
            env::set_var(key, value);
            Self {
                key: key.to_string(),
                previous,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(ref value) = self.previous {
                env::set_var(&self.key, value);
            } else {
                env::remove_var(&self.key);
            }
        }
    }

    #[test]
    fn publish_to_directory_copies_bundle_and_manifest() {
        let archive_dir = tempdir().expect("archive dir");
        let run_dir = archive_dir.path().join("run-1234");
        fs::create_dir_all(&run_dir).expect("run dir");
        let manifest_path = run_dir.join("manifest.json");
        let latest_path = archive_dir.path().join("latest.json");
        let bundle_path = run_dir.join("run-1234.zip");
        fs::write(&manifest_path, b"{\"manifest\":true}").expect("manifest write");
        fs::write(&latest_path, b"{\"latest\":true}").expect("latest write");
        fs::write(&bundle_path, b"bundle-bytes").expect("bundle write");

        let outcome = ChaosArchiveOutcome {
            run_id: "run-1234".to_string(),
            manifest_path: manifest_path.clone(),
            latest_path: latest_path.clone(),
            bundle_path: Some(bundle_path.clone()),
            bundle_file: Some("run-1234.zip".to_string()),
            bundle_digest: Some("abcd".to_string()),
        };

        let publish_dir = tempdir().expect("publish dir");
        let _guard = EnvGuard::set(
            "TB_CHAOS_ARCHIVE_PUBLISH_DIR",
            publish_dir.path().to_string_lossy().as_ref(),
        );

        publish_to_directory(&outcome).expect("directory publish");

        let copied_manifest = publish_dir.path().join("run-1234").join("manifest.json");
        assert_eq!(
            fs::read(&copied_manifest).expect("copied manifest"),
            fs::read(&manifest_path).expect("source manifest")
        );
        let copied_latest = publish_dir.path().join("latest.json");
        assert_eq!(
            fs::read(&copied_latest).expect("copied latest"),
            fs::read(&latest_path).expect("source latest")
        );
        let copied_bundle = publish_dir.path().join("run-1234.zip");
        assert_eq!(
            fs::read(&copied_bundle).expect("copied bundle"),
            fs::read(&bundle_path).expect("source bundle")
        );
    }

    #[test]
    fn upload_archive_objects_with_builds_expected_keys() {
        let mut calls = Vec::new();
        let receipts = upload_archive_objects_with(
            "audit",
            "providers",
            "run-20251027T190600Z",
            b"manifest-bytes",
            b"latest-bytes",
            Some(b"bundle-bytes"),
            Some("run-20251027T190600Z.zip"),
            3,
            Some(1_700_000_000),
            |bucket, key, payload, label, retries, fixed| {
                calls.push((
                    bucket.to_string(),
                    key.to_string(),
                    payload.to_vec(),
                    label.to_string(),
                    retries,
                    fixed,
                ));
                Ok(UploadReceipt {
                    payload_sha256: format!("digest-{label}"),
                })
            },
        )
        .expect("upload succeeds");

        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "audit");
        assert_eq!(calls[0].1, "providers/run-20251027T190600Z/manifest.json");
        assert_eq!(calls[0].3, "manifest");
        assert_eq!(calls[1].1, "providers/latest.json");
        assert_eq!(calls[2].1, "providers/run-20251027T190600Z.zip");
        assert_eq!(calls[2].3, "bundle");
        assert!(calls.iter().all(|call| call.4 == 3));
        assert!(calls.iter().all(|call| call.5 == Some(1_700_000_000)));

        assert_eq!(receipts.manifest.payload_sha256, "digest-manifest");
        assert_eq!(receipts.latest.payload_sha256, "digest-latest");
        let bundle = receipts.bundle.expect("bundle receipt");
        assert_eq!(bundle.0, "providers/run-20251027T190600Z.zip");
        assert_eq!(bundle.1.payload_sha256, "digest-bundle");
    }

    #[test]
    fn upload_with_retries_with_handles_retry_limit() {
        let mut attempts = Vec::new();
        let result =
            upload_with_retries_with("manifest", 3, Some(1_700_000_000), |attempt, total, now| {
                attempts.push((attempt, total, now.unix_timestamp().unwrap()));
                if attempt < 2 {
                    Err(UploadError::UnexpectedResponse {
                        status: 500,
                        body: "error".to_string(),
                    })
                } else {
                    Ok(UploadReceipt {
                        payload_sha256: "success".to_string(),
                    })
                }
            })
            .expect("retry succeeds");

        assert_eq!(result.payload_sha256, "success");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].0, 1);
        assert_eq!(attempts[0].1, 3);
        assert_eq!(attempts[1].0, 2);
        assert_eq!(attempts[1].1, 3);
        assert!(attempts.iter().all(|entry| entry.2 == 1_700_000_000));
    }
}

#[derive(Clone)]
struct ArchivedArtifact {
    file: String,
    blake3: String,
}

#[derive(Clone)]
struct ArchivedBundle {
    file: String,
    blake3: String,
    size: u64,
}

struct ChaosArchiveManifest {
    issued_at: u64,
    run_id: String,
    label: Option<String>,
    artifacts: BTreeMap<String, ArchivedArtifact>,
    bundle: Option<ArchivedBundle>,
}

struct ChaosArchiveLatest {
    issued_at: u64,
    run_id: String,
    label: Option<String>,
    manifest: String,
    bundle: Option<String>,
}

impl ArchivedArtifact {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("file".into(), Value::String(self.file.clone()));
        map.insert("blake3".into(), Value::String(self.blake3.clone()));
        Value::Object(map)
    }
}

impl ArchivedBundle {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("file".into(), Value::String(self.file.clone()));
        map.insert("blake3".into(), Value::String(self.blake3.clone()));
        map.insert("size".into(), Value::Number(Number::from(self.size)));
        Value::Object(map)
    }
}

impl ChaosArchiveManifest {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "issued_at".into(),
            Value::Number(Number::from(self.issued_at)),
        );
        map.insert("run_id".into(), Value::String(self.run_id.clone()));
        if let Some(label) = &self.label {
            map.insert("label".into(), Value::String(label.clone()));
        }
        let mut artifacts = Map::new();
        for (name, artifact) in &self.artifacts {
            artifacts.insert(name.clone(), artifact.to_value());
        }
        map.insert("artifacts".into(), Value::Object(artifacts));
        if let Some(bundle) = &self.bundle {
            map.insert("bundle".into(), bundle.to_value());
        }
        Value::Object(map)
    }
}

impl ChaosArchiveLatest {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "issued_at".into(),
            Value::Number(Number::from(self.issued_at)),
        );
        map.insert("run_id".into(), Value::String(self.run_id.clone()));
        if let Some(label) = &self.label {
            map.insert("label".into(), Value::String(label.clone()));
        }
        map.insert("manifest".into(), Value::String(self.manifest.clone()));
        if let Some(bundle) = &self.bundle {
            map.insert("bundle".into(), Value::String(bundle.clone()));
        }
        Value::Object(map)
    }
}

fn persist_status_snapshot(
    path: &str,
    snapshots: &[ChaosReadinessSnapshot],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(
        snapshots
            .iter()
            .map(|snapshot| snapshot.to_value())
            .collect(),
    );
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

fn load_status_snapshot(path: &str) -> Result<Vec<ChaosReadinessSnapshot>, Box<dyn Error>> {
    let data = fs::read(path)?;
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let payload: Value = json::from_slice(&data)?;
    let snapshots =
        decode_status_payload(payload).map_err(|err| Box::new(err) as Box<dyn Error>)?;
    Ok(snapshots)
}

fn persist_status_diff(path: &str, diffs: &[StatusDiffEntry]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(diffs.iter().map(StatusDiffEntry::to_value).collect());
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

struct OverlayReadinessRecord {
    scenario: String,
    module: String,
    site: String,
    provider: String,
    readiness: f64,
    scenario_readiness: f64,
    readiness_before: Option<f64>,
    provider_before: Option<String>,
    window_start: u64,
    window_end: u64,
    issued_at: u64,
    breaches: u64,
    sla_threshold: f64,
}

impl OverlayReadinessRecord {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert("module".into(), Value::String(self.module.clone()));
        map.insert("site".into(), Value::String(self.site.clone()));
        map.insert("provider".into(), Value::String(self.provider.clone()));
        map.insert("readiness".into(), Value::from(self.readiness));
        map.insert(
            "scenario_readiness".into(),
            Value::from(self.scenario_readiness),
        );
        if let Some(value) = self.readiness_before {
            map.insert("readiness_before".into(), Value::from(value));
        }
        if let Some(provider) = &self.provider_before {
            map.insert("provider_before".into(), Value::String(provider.clone()));
        }
        map.insert("window_start".into(), Value::from(self.window_start));
        map.insert("window_end".into(), Value::from(self.window_end));
        map.insert("issued_at".into(), Value::from(self.issued_at));
        map.insert("breaches".into(), Value::from(self.breaches));
        map.insert("sla_threshold".into(), Value::from(self.sla_threshold));
        Value::Object(map)
    }
}

struct ProviderDrillScenarioReport {
    scenario: String,
    module: ChaosModule,
    impacted_sites: usize,
    readiness_before: f64,
    readiness_after: f64,
    diff_entries: usize,
}

impl ProviderDrillScenarioReport {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert(
            "module".into(),
            Value::String(self.module.as_str().to_string()),
        );
        map.insert(
            "impacted_sites".into(),
            Value::from(self.impacted_sites as u64),
        );
        map.insert(
            "readiness_before".into(),
            Value::from(self.readiness_before),
        );
        map.insert("readiness_after".into(), Value::from(self.readiness_after));
        map.insert("diff_entries".into(), Value::from(self.diff_entries as u64));
        Value::Object(map)
    }
}

struct ProviderDrillReport {
    provider: String,
    scenarios: Vec<ProviderDrillScenarioReport>,
    total_diff_entries: usize,
}

impl ProviderDrillReport {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("provider".into(), Value::String(self.provider.clone()));
        map.insert(
            "total_diff_entries".into(),
            Value::from(self.total_diff_entries as u64),
        );
        map.insert(
            "scenarios".into(),
            Value::Array(self.scenarios.iter().map(|s| s.to_value()).collect()),
        );
        Value::Object(map)
    }
}

struct ProviderDrillOutcome {
    reports: Vec<ProviderDrillReport>,
    failures: Vec<String>,
}

fn persist_overlay_readiness(
    path: &str,
    snapshots: &[ChaosReadinessSnapshot],
    baseline: Option<&[ChaosReadinessSnapshot]>,
) -> Result<usize, Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let baseline_map = baseline.map(snapshot_map);
    let mut records = Vec::new();

    for snapshot in snapshots
        .iter()
        .filter(|entry| entry.module == ChaosModule::Overlay)
    {
        let baseline_sites = baseline_map
            .as_ref()
            .and_then(|map| map.get(&(snapshot.scenario.clone(), snapshot.module)));
        for site in &snapshot.site_readiness {
            let (readiness_before, provider_before) = baseline_sites
                .and_then(|summary| summary.sites.get(&site.site))
                .map(|value| {
                    (
                        Some(value.readiness),
                        Some(value.provider_kind.as_str().to_string()),
                    )
                })
                .unwrap_or((None, None));
            records.push(OverlayReadinessRecord {
                scenario: snapshot.scenario.clone(),
                module: snapshot.module.as_str().to_string(),
                site: site.site.clone(),
                provider: site.provider_kind.as_str().to_string(),
                readiness: site.readiness,
                scenario_readiness: snapshot.readiness,
                readiness_before,
                provider_before,
                window_start: snapshot.window_start,
                window_end: snapshot.window_end,
                issued_at: snapshot.issued_at,
                breaches: snapshot.breaches,
                sla_threshold: snapshot.sla_threshold,
            });
        }
    }

    records.sort_by(|a, b| {
        a.scenario
            .cmp(&b.scenario)
            .then(a.module.cmp(&b.module))
            .then(a.site.cmp(&b.site))
    });

    let mut file = File::create(path)?;
    let payload = Value::Array(
        records
            .iter()
            .map(OverlayReadinessRecord::to_value)
            .collect(),
    );
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(records.len())
}

fn provider_failover_reports(
    baseline: &[ChaosReadinessSnapshot],
    current: &[ChaosReadinessSnapshot],
) -> ProviderDrillOutcome {
    let providers = collect_overlay_providers(current);
    let baseline_map = snapshot_map(baseline);
    let current_map = snapshot_map(current);
    let mut reports = Vec::new();
    let mut failures = Vec::new();

    for provider in providers {
        let (mutated, impacted) = synthesize_provider_failover(current, provider);
        if impacted.is_empty() {
            reports.push(ProviderDrillReport {
                provider: provider.as_str().to_string(),
                scenarios: Vec::new(),
                total_diff_entries: 0,
            });
            continue;
        }
        let failover_map = snapshot_map(&mutated);
        let diffs = compute_status_diff(baseline, &mutated);
        let mut scenarios = Vec::new();
        for ((scenario, module), site_count) in impacted {
            let diff_entries = diffs
                .iter()
                .filter(|entry| entry.module == module && entry.scenario == scenario)
                .count();
            let readiness_before = current_map
                .get(&(scenario.clone(), module))
                .map(|summary| summary.readiness)
                .or_else(|| {
                    baseline_map
                        .get(&(scenario.clone(), module))
                        .map(|summary| summary.readiness)
                })
                .unwrap_or(1.0);
            let readiness_after = failover_map
                .get(&(scenario.clone(), module))
                .map(|summary| summary.readiness)
                .unwrap_or(readiness_before);
            scenarios.push(ProviderDrillScenarioReport {
                scenario: scenario.clone(),
                module,
                impacted_sites: site_count,
                readiness_before,
                readiness_after,
                diff_entries,
            });
            if diff_entries == 0 {
                failures.push(format!(
                    "provider '{}' failover did not register diff for scenario '{}'",
                    provider.as_str(),
                    scenario
                ));
            } else if !(readiness_after + STATUS_EPSILON < readiness_before) {
                failures.push(format!(
                    "provider '{}' failover for scenario '{}' did not lower readiness (before {:.4} after {:.4})",
                    provider.as_str(),
                    scenario,
                    readiness_before,
                    readiness_after
                ));
            }
        }
        scenarios.sort_by(|a, b| a.scenario.cmp(&b.scenario));
        let total_diff_entries = diffs
            .iter()
            .filter(|entry| entry.module == ChaosModule::Overlay)
            .count();
        if total_diff_entries == 0 {
            failures.push(format!(
                "provider '{}' failover produced no chaos/status diff entries",
                provider.as_str()
            ));
        }
        reports.push(ProviderDrillReport {
            provider: provider.as_str().to_string(),
            scenarios,
            total_diff_entries,
        });
    }

    reports.sort_by(|a, b| a.provider.cmp(&b.provider));
    ProviderDrillOutcome { reports, failures }
}

fn persist_provider_failover_reports(
    path: &str,
    reports: &[ProviderDrillReport],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(reports.iter().map(|r| r.to_value()).collect());
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

fn collect_overlay_providers(snapshots: &[ChaosReadinessSnapshot]) -> Vec<ChaosProviderKind> {
    let mut providers = HashSet::new();
    for snapshot in snapshots
        .iter()
        .filter(|entry| entry.module == ChaosModule::Overlay)
    {
        for site in &snapshot.site_readiness {
            providers.insert(site.provider_kind);
        }
    }
    let mut providers: Vec<_> = providers.into_iter().collect();
    providers.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    providers
}

fn synthesize_provider_failover(
    snapshots: &[ChaosReadinessSnapshot],
    provider: ChaosProviderKind,
) -> (
    Vec<ChaosReadinessSnapshot>,
    HashMap<(String, ChaosModule), usize>,
) {
    let mut mutated = snapshots.to_vec();
    let mut impacted: HashMap<(String, ChaosModule), usize> = HashMap::new();
    for snapshot in &mut mutated {
        if snapshot.module != ChaosModule::Overlay {
            continue;
        }
        let mut count = 0usize;
        for site in &mut snapshot.site_readiness {
            if site.provider_kind == provider {
                site.readiness = 0.0;
                count = count.saturating_add(1);
            }
        }
        if count > 0 {
            snapshot.readiness = snapshot
                .site_readiness
                .iter()
                .map(|site| site.readiness)
                .fold(1.0, f64::min);
            snapshot.breaches = snapshot.breaches.saturating_add(1);
            impacted.insert((snapshot.scenario.clone(), snapshot.module), count);
        }
    }
    (mutated, impacted)
}

fn fetch_status_snapshot(endpoint: &str) -> Result<Vec<ChaosReadinessSnapshot>, Box<dyn Error>> {
    let client = BlockingClient::default();
    let response = client
        .request(Method::Get, endpoint)?
        .timeout(STATUS_FETCH_TIMEOUT)
        .send()?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("chaos/status fetch failed with status {}", status.as_u16()).into());
    }
    let body = response.into_body();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let payload: Value = json::from_slice(&body)?;
    let snapshots =
        decode_status_payload(payload).map_err(|err| Box::new(err) as Box<dyn Error>)?;
    Ok(snapshots)
}

fn decode_status_payload(
    value: Value,
) -> Result<Vec<ChaosReadinessSnapshot>, ChaosSnapshotDecodeError> {
    ChaosReadinessSnapshot::decode_array(&value)
}

const STATUS_EPSILON: f64 = 1e-6;

fn compute_status_diff(
    baseline: &[ChaosReadinessSnapshot],
    current: &[ChaosReadinessSnapshot],
) -> Vec<StatusDiffEntry> {
    let baseline_map = snapshot_map(baseline);
    let current_map = snapshot_map(current);
    let mut keys: HashSet<(String, ChaosModule)> = HashSet::new();
    keys.extend(baseline_map.keys().cloned());
    keys.extend(current_map.keys().cloned());
    let mut keys: Vec<_> = keys.into_iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.as_str().cmp(b.1.as_str())));

    let mut diffs = Vec::new();
    for (scenario, module) in keys {
        let baseline_entry = baseline_map.get(&(scenario.clone(), module));
        let current_entry = current_map.get(&(scenario.clone(), module));
        let readiness_before = baseline_entry.map(|entry| entry.readiness);
        let readiness_after = current_entry.map(|entry| entry.readiness);

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        match (baseline_entry, current_entry) {
            (Some(before), Some(after)) => {
                for (site, value) in &after.sites {
                    match before.sites.get(site) {
                        Some(prev) => {
                            let readiness_changed = !approx_equal(prev.readiness, value.readiness);
                            let provider_changed = prev.provider_kind != value.provider_kind;
                            if readiness_changed || provider_changed {
                                changed.push(SiteChange {
                                    site: site.clone(),
                                    before: Some(prev.readiness),
                                    after: Some(value.readiness),
                                    provider_before: Some(prev.provider_kind),
                                    provider_after: Some(value.provider_kind),
                                });
                            }
                        }
                        None => added.push(SiteEntry {
                            site: site.clone(),
                            provider_kind: value.provider_kind,
                        }),
                    }
                }
                for (site, prev) in &before.sites {
                    if !after.sites.contains_key(site) {
                        removed.push(SiteEntry {
                            site: site.clone(),
                            provider_kind: prev.provider_kind,
                        });
                    }
                }
            }
            (Some(before), None) => {
                removed.extend(before.sites.iter().map(|(site, prev)| SiteEntry {
                    site: site.clone(),
                    provider_kind: prev.provider_kind,
                }));
            }
            (None, Some(after)) => {
                added.extend(after.sites.iter().map(|(site, value)| SiteEntry {
                    site: site.clone(),
                    provider_kind: value.provider_kind,
                }));
            }
            (None, None) => {}
        }

        let readiness_changed = match (readiness_before, readiness_after) {
            (Some(before), Some(after)) => !approx_equal(before, after),
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };

        if added.is_empty() && removed.is_empty() && changed.is_empty() && !readiness_changed {
            continue;
        }

        diffs.push(StatusDiffEntry {
            module,
            scenario,
            readiness_before,
            readiness_after,
            site_added: added,
            site_removed: removed,
            site_changed: changed,
        });
    }

    diffs.sort_by(|a, b| {
        a.scenario
            .cmp(&b.scenario)
            .then(a.module.as_str().cmp(b.module.as_str()))
    });
    diffs
}

fn approx_equal(lhs: f64, rhs: f64) -> bool {
    (lhs - rhs).abs() <= STATUS_EPSILON
}

#[derive(Clone)]
struct SiteSummary {
    readiness: f64,
    provider_kind: ChaosProviderKind,
}

#[derive(Clone)]
struct SnapshotSummary {
    readiness: f64,
    sites: HashMap<String, SiteSummary>,
}

fn snapshot_map(
    snapshots: &[ChaosReadinessSnapshot],
) -> HashMap<(String, ChaosModule), SnapshotSummary> {
    let mut map = HashMap::new();
    for snapshot in snapshots {
        let sites = snapshot
            .site_readiness
            .iter()
            .map(|entry| {
                (
                    entry.site.clone(),
                    SiteSummary {
                        readiness: entry.readiness,
                        provider_kind: entry.provider_kind,
                    },
                )
            })
            .collect();
        map.insert(
            (snapshot.scenario.clone(), snapshot.module),
            SnapshotSummary {
                readiness: snapshot.readiness,
                sites,
            },
        );
    }
    map
}

struct StatusDiffEntry {
    module: ChaosModule,
    scenario: String,
    readiness_before: Option<f64>,
    readiness_after: Option<f64>,
    site_added: Vec<SiteEntry>,
    site_removed: Vec<SiteEntry>,
    site_changed: Vec<SiteChange>,
}

impl StatusDiffEntry {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert(
            "module".into(),
            Value::String(self.module.as_str().to_string()),
        );
        if let Some(value) = self.readiness_before {
            map.insert("readiness_before".into(), Value::from(value));
        }
        if let Some(value) = self.readiness_after {
            map.insert("readiness_after".into(), Value::from(value));
        }
        map.insert(
            "site_added".into(),
            Value::Array(self.site_added.iter().map(SiteEntry::to_value).collect()),
        );
        map.insert(
            "site_removed".into(),
            Value::Array(self.site_removed.iter().map(SiteEntry::to_value).collect()),
        );
        map.insert(
            "site_changed".into(),
            Value::Array(self.site_changed.iter().map(SiteChange::to_value).collect()),
        );
        Value::Object(map)
    }
}

struct SiteEntry {
    site: String,
    provider_kind: ChaosProviderKind,
}

impl SiteEntry {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("site".into(), Value::String(self.site.clone()));
        map.insert(
            "provider_kind".into(),
            Value::String(self.provider_kind.as_str().into()),
        );
        Value::Object(map)
    }
}

struct SiteChange {
    site: String,
    before: Option<f64>,
    after: Option<f64>,
    provider_before: Option<ChaosProviderKind>,
    provider_after: Option<ChaosProviderKind>,
}

impl SiteChange {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("site".into(), Value::String(self.site.clone()));
        if let Some(value) = self.before {
            map.insert("before".into(), Value::from(value));
        }
        if let Some(value) = self.after {
            map.insert("after".into(), Value::from(value));
        }
        if let Some(provider) = self.provider_before {
            map.insert(
                "provider_before".into(),
                Value::String(provider.as_str().into()),
            );
        }
        if let Some(provider) = self.provider_after {
            map.insert(
                "provider_after".into(),
                Value::String(provider.as_str().into()),
            );
        }
        Value::Object(map)
    }
}

fn format_modules(attestations: &[ChaosAttestation]) -> String {
    let mut modules: Vec<&'static str> =
        attestations.iter().map(|att| att.module.as_str()).collect();
    modules.sort();
    modules.dedup();
    modules.join(",")
}

fn apply_site_overrides(sim: &mut Simulation) {
    let Ok(spec) = env::var("TB_CHAOS_SITE_TOPOLOGY") else {
        return;
    };
    match parse_site_topology(&spec) {
        Ok(map) => {
            let harness = sim.chaos_harness_mut();
            for (module, sites) in map {
                harness.configure_sites(module, sites);
            }
        }
        Err(err) => {
            eprintln!("[chaos-lab] invalid TB_CHAOS_SITE_TOPOLOGY: {err}");
        }
    }
}

fn parse_site_topology(spec: &str) -> Result<HashMap<ChaosModule, Vec<ChaosSite>>, String> {
    let mut map: HashMap<ChaosModule, Vec<ChaosSite>> = HashMap::new();
    for module_entry in spec.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let mut parts = module_entry.splitn(2, '=');
        let module_key = parts
            .next()
            .ok_or_else(|| "missing module identifier".to_string())?
            .trim();
        let sites_spec = parts
            .next()
            .ok_or_else(|| format!("missing site list for module '{module_key}'"))?
            .trim();
        let Some(module) = ChaosModule::from_str(module_key) else {
            return Err(format!("unknown chaos module '{module_key}'"));
        };
        let mut sites = Vec::new();
        for site_entry in sites_spec
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut fields = site_entry.split(':');
            let name = fields
                .next()
                .ok_or_else(|| format!("invalid site entry '{site_entry}'"))?
                .trim();
            let weight_str = fields.next().unwrap_or("1.0").trim();
            let latency_str = fields.next().unwrap_or("0.0").trim();
            let provider_str = fields.next().unwrap_or("").trim();
            let weight = weight_str
                .parse::<f64>()
                .map_err(|_| format!("invalid weight '{weight_str}' for site '{name}'"))?;
            let latency = latency_str
                .parse::<f64>()
                .map_err(|_| format!("invalid latency '{latency_str}' for site '{name}'"))?;
            let provider_kind = if provider_str.is_empty() {
                ChaosProviderKind::Unknown
            } else {
                ChaosProviderKind::from_str(provider_str).ok_or_else(|| {
                    format!("invalid provider kind '{provider_str}' for site '{name}'")
                })?
            };
            sites.push(ChaosSite::with_kind(name, weight, latency, provider_kind));
        }
        if !sites.is_empty() {
            map.entry(module).or_default().extend(sites);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitoring_build::ChaosSiteReadiness;
    use sys::tempfile;

    fn snapshot(
        module: ChaosModule,
        scenario: &str,
        readiness: f64,
        sites: &[(&str, f64, ChaosProviderKind)],
    ) -> ChaosReadinessSnapshot {
        ChaosReadinessSnapshot {
            scenario: scenario.to_string(),
            module,
            readiness,
            sla_threshold: 0.9,
            breaches: 0,
            window_start: 0,
            window_end: 1,
            issued_at: 1,
            signer: [0u8; 32],
            digest: [0u8; 32],
            site_readiness: sites
                .iter()
                .map(|(name, value, provider)| ChaosSiteReadiness {
                    site: (*name).to_string(),
                    readiness: *value,
                    provider_kind: *provider,
                })
                .collect(),
        }
    }

    #[test]
    fn diff_detects_removed_and_changed_sites() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.9,
            &[
                ("site-a", 0.9, ChaosProviderKind::Foundation),
                ("site-b", 0.88, ChaosProviderKind::Partner),
            ],
        )];
        let current = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.92,
            &[("site-b", 0.91, ChaosProviderKind::Partner)],
        )];
        let diffs = compute_status_diff(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        let entry = &diffs[0];
        assert_eq!(entry.scenario, "overlay-test");
        assert_eq!(entry.module, ChaosModule::Overlay);
        assert!(entry.site_added.is_empty());
        assert_eq!(entry.site_removed.len(), 1);
        assert_eq!(entry.site_removed[0].site, "site-a");
        assert_eq!(
            entry.site_removed[0].provider_kind,
            ChaosProviderKind::Foundation
        );
        assert_eq!(entry.site_changed.len(), 1);
        assert_eq!(entry.site_changed[0].site, "site-b");
        assert_eq!(entry.site_changed[0].before, Some(0.88));
        assert_eq!(entry.site_changed[0].after, Some(0.91));
        assert_eq!(
            entry.site_changed[0].provider_before,
            Some(ChaosProviderKind::Partner)
        );
        assert_eq!(
            entry.site_changed[0].provider_after,
            Some(ChaosProviderKind::Partner)
        );
        assert_eq!(entry.readiness_before, Some(0.9));
        assert_eq!(entry.readiness_after, Some(0.92));
    }

    #[test]
    fn diff_ignores_identical_snapshots() {
        let baseline = vec![snapshot(
            ChaosModule::Compute,
            "compute-test",
            0.95,
            &[("site-a", 0.95, ChaosProviderKind::Unknown)],
        )];
        let current = baseline.clone();
        let diffs = compute_status_diff(&baseline, &current);
        assert!(diffs.is_empty());
    }

    #[test]
    fn diff_flags_readiness_only_changes() {
        let baseline = vec![snapshot(ChaosModule::Storage, "storage-test", 0.85, &[])];
        let current = vec![snapshot(ChaosModule::Storage, "storage-test", 0.8, &[])];
        let diffs = compute_status_diff(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        let entry = &diffs[0];
        assert!(entry.site_added.is_empty());
        assert!(entry.site_removed.is_empty());
        assert_eq!(entry.site_changed.len(), 0);
        assert_eq!(entry.readiness_before, Some(0.85));
        assert_eq!(entry.readiness_after, Some(0.8));
    }

    #[test]
    fn diff_detects_provider_changes() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.9,
            &[("site-a", 0.9, ChaosProviderKind::Foundation)],
        )];
        let current = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-test",
            0.9,
            &[("site-a", 0.9, ChaosProviderKind::Partner)],
        )];
        let diffs = compute_status_diff(&baseline, &current);
        assert_eq!(diffs.len(), 1);
        let entry = &diffs[0];
        assert!(entry.site_added.is_empty());
        assert!(entry.site_removed.is_empty());
        assert_eq!(entry.site_changed.len(), 1);
        let change = &entry.site_changed[0];
        assert_eq!(change.site, "site-a");
        assert_eq!(change.before, Some(0.9));
        assert_eq!(change.after, Some(0.9));
        assert_eq!(change.provider_before, Some(ChaosProviderKind::Foundation));
        assert_eq!(change.provider_after, Some(ChaosProviderKind::Partner));
    }

    #[test]
    fn overlay_readiness_serializes_records() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.91,
            &[
                ("site-a", 0.9, ChaosProviderKind::Foundation),
                ("site-b", 0.88, ChaosProviderKind::Partner),
            ],
        )];
        let current = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.93,
            &[
                ("site-a", 0.92, ChaosProviderKind::Foundation),
                ("site-c", 0.87, ChaosProviderKind::Community),
            ],
        )];
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("overlay.json");
        let count =
            persist_overlay_readiness(path.to_str().expect("utf8 path"), &current, Some(&baseline))
                .expect("persist overlay readiness");
        assert_eq!(count, 2);
        let data = fs::read(&path).expect("overlay data");
        let value: Value = json::from_slice(&data).expect("overlay json");
        let entries = value.as_array().expect("entries array");
        assert_eq!(entries.len(), 2);
        let site_a = entries
            .iter()
            .find(|entry| entry.get("site").and_then(Value::as_str) == Some("site-a"))
            .expect("site-a entry");
        assert_eq!(
            site_a.get("provider").and_then(Value::as_str),
            Some("foundation")
        );
        assert_eq!(
            site_a.get("readiness_before").and_then(Value::as_f64),
            Some(0.9)
        );
        let site_c = entries
            .iter()
            .find(|entry| entry.get("site").and_then(Value::as_str) == Some("site-c"))
            .expect("site-c entry");
        assert!(site_c.get("readiness_before").is_none());
        assert_eq!(
            site_c.get("provider").and_then(Value::as_str),
            Some("community")
        );
    }

    #[test]
    fn fetch_status_snapshot_downloads_data() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let snapshots = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.94,
            &[("site-a", 0.94, ChaosProviderKind::Foundation)],
        )];
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let payload = json::to_vec(&Value::Array(
            snapshots
                .iter()
                .map(ChaosReadinessSnapshot::to_value)
                .collect(),
        ))
        .expect("serialize snapshots");
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&payload);
            }
        });

        let endpoint = format!("http://{}/chaos/status", addr);
        let fetched = fetch_status_snapshot(&endpoint).expect("fetch status");
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].scenario, snapshots[0].scenario);
        assert_eq!(fetched[0].module, snapshots[0].module);
        assert_eq!(fetched[0].site_readiness.len(), 1);
        handle.join().expect("server thread");
    }

    #[test]
    fn provider_failover_reports_detects_outage() {
        let baseline = vec![snapshot(
            ChaosModule::Overlay,
            "overlay-soak",
            0.95,
            &[
                ("site-a", 0.95, ChaosProviderKind::Foundation),
                ("site-b", 0.92, ChaosProviderKind::Partner),
            ],
        )];
        let outcome = provider_failover_reports(&baseline, &baseline);
        assert!(
            outcome.failures.is_empty(),
            "failures: {:?}",
            outcome.failures
        );
        let report = outcome
            .reports
            .iter()
            .find(|report| report.provider == "foundation")
            .expect("foundation report");
        assert_eq!(report.total_diff_entries, 1);
        assert_eq!(report.scenarios.len(), 1);
        let scenario = &report.scenarios[0];
        assert_eq!(scenario.impacted_sites, 1);
        assert!(scenario.readiness_after + STATUS_EPSILON < scenario.readiness_before);
    }
}

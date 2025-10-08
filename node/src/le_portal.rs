use crypto_suite::hashing::blake3;
use foundation_serialization::json;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(all(feature = "telemetry", feature = "privacy"))]
use crate::telemetry::PRIVACY_SANITIZATION_TOTAL;
#[cfg(feature = "privacy")]
use privacy::redaction;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LeRequest {
    pub timestamp: u64,
    pub agency: String,
    pub case_hash: String,
    pub jurisdiction: String,
    #[serde(default)]
    pub language: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LeAction {
    pub timestamp: u64,
    pub agency: String,
    pub action_hash: String,
    pub jurisdiction: String,
    #[serde(default)]
    pub language: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub timestamp: u64,
    pub agency: String,
    pub case_hash: String,
    pub evidence_hash: String,
    pub jurisdiction: String,
    #[serde(default)]
    pub language: String,
}

fn log_path(base: &str, file: &str) -> std::path::PathBuf {
    std::path::Path::new(base).join(file)
}

pub fn record_request(
    base: &str,
    agency: &str,
    case_id: &str,
    jurisdiction: &str,
    language: &str,
) -> std::io::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let case_hash = blake3::hash(case_id.as_bytes()).to_hex().to_string();
    let entry = LeRequest {
        timestamp: ts,
        agency: agency.to_string(),
        case_hash: case_hash.clone(),
        jurisdiction: jurisdiction.to_string(),
        language: language.to_string(),
    };
    let line = json::to_string(&entry).unwrap_or_else(|e| panic!("serialize LE request: {e}"));
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path(base, "le_requests.log"))?;
    writeln!(file, "{}", line)?;
    let _ = state::append_audit(std::path::Path::new(base), &line);
    Ok(case_hash)
}

pub fn list_requests(base: &str) -> std::io::Result<Vec<LeRequest>> {
    let path = log_path(base, "le_requests.log");
    let data = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in data.lines() {
        if line.is_empty() {
            continue;
        }
        if let Ok(req) = json::from_str::<LeRequest>(line) {
            out.push(req);
        }
    }
    Ok(out)
}

pub fn record_action(
    base: &str,
    agency: &str,
    action: &str,
    jurisdiction: &str,
    language: &str,
) -> std::io::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let action_hash = blake3::hash(action.as_bytes()).to_hex().to_string();
    let entry = LeAction {
        timestamp: ts,
        agency: agency.to_string(),
        action_hash: action_hash.clone(),
        jurisdiction: jurisdiction.to_string(),
        language: language.to_string(),
    };
    let line = json::to_string(&entry).unwrap_or_else(|e| panic!("serialize LE action: {e}"));
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path(base, "le_actions.log"))?;
    writeln!(file, "{}", line)?;
    let _ = state::append_audit(std::path::Path::new(base), &line);
    Ok(action_hash)
}

pub fn list_actions(base: &str) -> std::io::Result<Vec<LeAction>> {
    let path = log_path(base, "le_actions.log");
    let data = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in data.lines() {
        if line.is_empty() {
            continue;
        }
        if let Ok(act) = json::from_str::<LeAction>(line) {
            out.push(act);
        }
    }
    Ok(out)
}

pub fn record_canary(base: &str, message: &str) -> std::io::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hash = blake3::hash(message.as_bytes()).to_hex().to_string();
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path(base, "warrant_canary.log"))?;
    writeln!(file, "{} {}", ts, hash)?;
    Ok(hash)
}

pub fn record_evidence(
    base: &str,
    agency: &str,
    case_id: &str,
    jurisdiction: &str,
    language: &str,
    data: &[u8],
) -> std::io::Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let case_hash = blake3::hash(case_id.as_bytes()).to_hex().to_string();
    let evidence_hash = blake3::hash(data).to_hex().to_string();
    let dir = std::path::Path::new(base).join("evidence");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(&evidence_hash), data)?;
    let entry = EvidenceRecord {
        timestamp: ts,
        agency: agency.to_string(),
        case_hash: case_hash.clone(),
        evidence_hash: evidence_hash.clone(),
        jurisdiction: jurisdiction.to_string(),
        language: language.to_string(),
    };
    let line = json::to_string(&entry).unwrap_or_else(|e| panic!("serialize evidence: {e}"));
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path(base, "le_evidence.log"))?;
    writeln!(file, "{}", line)?;
    let _ = state::append_audit(std::path::Path::new(base), &line);
    Ok(evidence_hash)
}

pub fn list_evidence(base: &str) -> std::io::Result<Vec<EvidenceRecord>> {
    let path = log_path(base, "le_evidence.log");
    let data = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in data.lines() {
        if line.is_empty() {
            continue;
        }
        if let Ok(ev) = json::from_str::<EvidenceRecord>(line) {
            out.push(ev);
        }
    }
    Ok(out)
}

/// Sanitize sensitive memo fields according to jurisdiction policy. Returns
/// true if the payload was modified.
#[cfg(feature = "privacy")]
pub fn sanitize_payload(memo: &mut String, jurisdiction: &str) -> bool {
    let changed = {
        // Simple policy: disallow memos for non-local jurisdictions.
        let allowed = jurisdiction == "local";
        redaction::redact_memo(memo, allowed)
    };
    if changed {
        #[cfg(feature = "telemetry")]
        PRIVACY_SANITIZATION_TOTAL.inc();
    }
    changed
}

#[cfg(not(feature = "privacy"))]
pub fn sanitize_payload(_memo: &mut String, _jurisdiction: &str) -> bool {
    false
}

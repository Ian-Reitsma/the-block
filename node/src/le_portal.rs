use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LeRequest {
    pub timestamp: u64,
    pub agency: String,
    pub case_hash: String,
    pub jurisdiction: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LeAction {
    pub timestamp: u64,
    pub agency: String,
    pub action_hash: String,
    pub jurisdiction: String,
}

fn log_path(base: &str, file: &str) -> std::path::PathBuf {
    std::path::Path::new(base).join(file)
}

pub fn record_request(
    base: &str,
    agency: &str,
    case_id: &str,
    jurisdiction: &str,
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
    };
    let line =
        serde_json::to_string(&entry).unwrap_or_else(|e| panic!("serialize LE request: {e}"));
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path(base, "le_requests.log"))?;
    writeln!(file, "{}", line)?;
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
        if let Ok(req) = serde_json::from_str::<LeRequest>(line) {
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
    };
    let line = serde_json::to_string(&entry).unwrap_or_else(|e| panic!("serialize LE action: {e}"));
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path(base, "le_actions.log"))?;
    writeln!(file, "{}", line)?;
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
        if let Ok(act) = serde_json::from_str::<LeAction>(line) {
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

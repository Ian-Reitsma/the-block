use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Result as IoResult, Seek, SeekFrom},
    path::{Path, PathBuf},
    thread::sleep,
    time::Duration,
};

use diagnostics::anyhow::Result;
use foundation_serialization::{json, Deserialize, Serialize};
use the_block::{gateway::read_receipt, ReadAck};

const DEFAULT_ACK_DIR: &str = "gateway_acks";
const STATE_FILE: &str = ".gateway_ack_ingest.json";
const DEFAULT_POLL_MS: u64 = 5_000;

#[derive(Debug)]
struct AckIngestConfig {
    ack_dir: PathBuf,
    state_path: PathBuf,
    poll_interval: Duration,
}

impl AckIngestConfig {
    fn from_env() -> Self {
        let ack_dir = env::var("TB_GATEWAY_ACK_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ACK_DIR));
        let state_path = ack_dir.join(STATE_FILE);
        let poll_interval = env::var("TB_GATEWAY_ACK_POLL_INTERVAL_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(DEFAULT_POLL_MS));
        Self {
            ack_dir,
            state_path,
            poll_interval,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct AckIngestState {
    offsets: HashMap<String, u64>,
}

impl AckIngestState {
    fn load(path: &Path) -> Self {
        match fs::read(path) {
            Ok(bytes) => json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => AckIngestState::default(),
        }
    }

    fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = json::to_string(self).map_err(|err| err.to_string())?;
        fs::write(path, serialized)?;
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let cfg = AckIngestConfig::from_env();
    let mut state = AckIngestState::load(&cfg.state_path);
    loop {
        if let Err(err) = process_ack_dir(&cfg, &mut state) {
            eprintln!("gateway-ack-ingest: error processing acks: {err}");
        }
        if let Err(err) = state.save(&cfg.state_path) {
            eprintln!("gateway-ack-ingest: failed to persist state: {err}");
        }
        sleep(cfg.poll_interval);
    }
}

fn process_ack_dir(cfg: &AckIngestConfig, state: &mut AckIngestState) -> Result<()> {
    if !cfg.ack_dir.exists() {
        fs::create_dir_all(&cfg.ack_dir)?;
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(&cfg.ack_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("jsonl"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let key = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let offset = *state.offsets.get(&key).unwrap_or(&0);
        if let Err(err) = process_ack_file(&path, offset, state, &key) {
            eprintln!("gateway-ack-ingest: failed to process {path:?}: {err}");
        }
    }
    Ok(())
}

fn process_ack_file(path: &Path, start: u64, state: &mut AckIngestState, key: &str) -> Result<()> {
    let file = OpenOptions::new().read(true).open(path)?;
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(start))?;
    let mut buffer = String::new();
    loop {
        buffer.clear();
        let read = reader.read_line(&mut buffer)?;
        if read == 0 {
            break;
        }
        let trimmed = buffer.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.is_empty() {
            continue;
        }
        match json::from_str::<ReadAck>(trimmed.as_ref()) {
            Ok(ack) => {
                if let Err(err) = ingest_ack(ack) {
                    eprintln!(
                        "gateway-ack-ingest: failed to append read receipt for {}: {err}",
                        trimmed
                    );
                }
            }
            Err(err) => {
                eprintln!("gateway-ack-ingest: failed to parse ack: {err} (line: {trimmed})");
            }
        }
    }
    if let Ok(position) = reader.stream_position() {
        state.offsets.insert(key.to_string(), position);
    }
    Ok(())
}

fn ingest_ack(ack: ReadAck) -> IoResult<()> {
    let provider_id = if ack.provider.is_empty() {
        ack.domain.clone()
    } else {
        ack.provider.clone()
    };
    let dynamic =
        ack.selection_receipt.is_some() || ack.campaign_id.is_some() || ack.creative_id.is_some();
    read_receipt::append_with_ts(&ack.domain, &provider_id, ack.bytes, dynamic, true, ack.ts)
}

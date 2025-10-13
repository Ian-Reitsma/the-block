use diagnostics::anyhow::{self, Result};
use foundation_serialization::Serialize;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use the_block::governance::{self, ApprovedRelease, GovStore};

const CACHE_TTL: Duration = Duration::from_secs(15);

struct CachedEntries {
    refreshed: Instant,
    entries: Vec<ReleaseHistoryEntry>,
}

static RELEASE_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedEntries>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReleaseHistoryEntry {
    pub build_hash: String,
    pub proposer: String,
    pub activated_epoch: u64,
    pub last_install_ts: Option<u64>,
    pub install_count: usize,
    pub signature_threshold: u32,
    pub signer_set: Vec<String>,
    pub attested_signers: Vec<String>,
    pub quorum_met: bool,
    pub install_times: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReleaseHistoryPage {
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    pub entries: Vec<ReleaseHistoryEntry>,
}

#[derive(Debug, Default, Clone)]
pub struct ReleaseHistoryFilter {
    pub proposer: Option<String>,
    pub start_epoch: Option<u64>,
    pub end_epoch: Option<u64>,
}

fn to_entry(
    record: ApprovedRelease,
    installs: &mut HashMap<String, Vec<u64>>,
) -> ReleaseHistoryEntry {
    let install_times = installs.remove(&record.build_hash).unwrap_or_default();
    let last_install_ts = install_times.last().copied();
    let attested_signers: Vec<String> = record
        .signatures
        .iter()
        .map(|att| att.signer.clone())
        .collect();
    let quorum_met = if record.signature_threshold == 0 {
        attested_signers.is_empty()
    } else {
        attested_signers.len() as u32 >= record.signature_threshold
    };
    ReleaseHistoryEntry {
        build_hash: record.build_hash,
        proposer: record.proposer,
        activated_epoch: record.activated_epoch,
        last_install_ts,
        install_count: install_times.len(),
        signature_threshold: record.signature_threshold,
        signer_set: record.signer_set,
        attested_signers,
        quorum_met,
        install_times,
    }
}

fn load_entries(path: &Path) -> Result<Vec<ReleaseHistoryEntry>> {
    let store = GovStore::open(path);
    let mut install_map: HashMap<String, Vec<u64>> =
        governance::controller::release_installations(&store)
            .map_err(anyhow::Error::from_error)?
            .into_iter()
            .collect();
    let mut entries: Vec<ReleaseHistoryEntry> = governance::controller::approved_releases(&store)
        .map_err(anyhow::Error::from_error)?
        .into_iter()
        .map(|record| to_entry(record, &mut install_map))
        .collect();
    entries.sort_by_key(|entry| Reverse(entry.activated_epoch));
    Ok(entries)
}

fn cached_entries(path: &Path) -> Result<Vec<ReleaseHistoryEntry>> {
    let cache = RELEASE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = path.to_path_buf();
    if let Some(entries) = cache.lock().unwrap().get(&key).and_then(|cached| {
        if cached.refreshed.elapsed() <= CACHE_TTL {
            Some(cached.entries.clone())
        } else {
            None
        }
    }) {
        return Ok(entries);
    }
    let entries = load_entries(path)?;
    cache.lock().unwrap().insert(
        key,
        CachedEntries {
            refreshed: Instant::now(),
            entries: entries.clone(),
        },
    );
    Ok(entries)
}

pub fn release_history(path: impl AsRef<Path>) -> Result<Vec<ReleaseHistoryEntry>> {
    cached_entries(path.as_ref())
}

pub fn paginated_release_history(
    path: impl AsRef<Path>,
    page: usize,
    page_size: usize,
    filter: ReleaseHistoryFilter,
) -> Result<ReleaseHistoryPage> {
    let entries = cached_entries(path.as_ref())?;
    let filtered: Vec<ReleaseHistoryEntry> = entries
        .into_iter()
        .filter(|entry| {
            filter
                .proposer
                .as_ref()
                .map(|p| entry.proposer.eq_ignore_ascii_case(p))
                .unwrap_or(true)
                && filter
                    .start_epoch
                    .map(|s| entry.activated_epoch >= s)
                    .unwrap_or(true)
                && filter
                    .end_epoch
                    .map(|e| entry.activated_epoch <= e)
                    .unwrap_or(true)
        })
        .collect();
    let total = filtered.len();
    let size = page_size.max(1);
    let start = page.saturating_mul(size);
    let end = (start + size).min(total);
    let page_entries = if start >= total {
        Vec::new()
    } else {
        filtered[start..end].to_vec()
    };
    Ok(ReleaseHistoryPage {
        total,
        page,
        page_size: size,
        entries: page_entries,
    })
}

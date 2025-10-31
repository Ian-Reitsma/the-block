#![forbid(unsafe_code)]

use crate::codec::serialize_contract_record;
use crate::legacy::{
    load_manifest_entries, manifest_status, migrated_manifest_path, pending_manifest_path,
    ManifestStatus,
};
use crate::{ContractRecord, Result, StorageMarketError};
use crypto_suite::hashing::blake3::Hasher;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::engine::{Engine, Tree};

const AUDIT_SAMPLE_LIMIT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    InsertMissing,
    OverwriteExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestSource {
    Auto,
    Pending,
    Migrated,
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestSummary {
    pub status: ManifestStatus,
    pub total_entries: usize,
    pub present: usize,
    pub missing: usize,
    pub duplicates: usize,
    pub source_path: Option<PathBuf>,
}

impl ManifestSummary {
    pub fn empty(status: ManifestStatus) -> Self {
        Self {
            status,
            total_entries: 0,
            present: 0,
            missing: 0,
            duplicates: 0,
            source_path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumScope {
    ContractsOnly,
    AllColumnFamilies,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumDigest {
    pub entries: usize,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumComparison {
    pub scope: ChecksumScope,
    pub manifest: Option<ChecksumDigest>,
    pub database: ChecksumDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntryStatus {
    pub key: Vec<u8>,
    pub present: bool,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditReport {
    pub summary: ManifestSummary,
    pub entries: Vec<AuditEntryStatus>,
    pub missing_keys: Vec<Vec<u8>>,
    pub duplicate_keys: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct ManifestScan {
    summary: ManifestSummary,
    entries: Vec<AuditEntryStatus>,
    missing_keys: Vec<Vec<u8>>,
    duplicate_keys: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStats {
    pub total_entries: usize,
    pub applied: usize,
    pub skipped_existing: usize,
    pub overwritten: usize,
    pub no_change: usize,
}

impl ImportStats {
    fn new(total_entries: usize) -> Self {
        Self {
            total_entries,
            applied: 0,
            skipped_existing: 0,
            overwritten: 0,
            no_change: 0,
        }
    }
}

pub struct StorageImporter {
    engine: Engine,
    contracts: Tree,
}

impl StorageImporter {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let engine = Engine::open(path.as_ref())?;
        let contracts = engine.open_tree("market/contracts")?;
        Ok(Self { engine, contracts })
    }

    pub fn base_path(&self) -> &Path {
        self.engine.base_path()
    }

    pub fn manifest_status(&self) -> ManifestStatus {
        manifest_status(self.base_path())
    }

    pub fn summarize(&self, source: ManifestSource) -> Result<ManifestSummary> {
        let scan = self.scan_manifest(source, true, None)?;
        Ok(scan.summary)
    }

    pub fn import(&self, source: ManifestSource, mode: ImportMode) -> Result<ImportStats> {
        let Some(path) = self.resolve_source_path(source, false)? else {
            return Err(StorageMarketError::LegacyManifest(
                "legacy manifest not found for import".into(),
            ));
        };
        let entries = load_manifest_entries(&path)?;
        apply_entries(&self.contracts, &entries, mode)
    }

    pub fn audit(&self, source: ManifestSource) -> Result<AuditReport> {
        let scan = self.scan_manifest(source, true, Some(AUDIT_SAMPLE_LIMIT))?;
        Ok(AuditReport {
            summary: scan.summary,
            entries: scan.entries,
            missing_keys: scan.missing_keys,
            duplicate_keys: scan.duplicate_keys,
        })
    }

    pub fn manifest_checksum(&self, source: ManifestSource) -> Result<Option<ChecksumDigest>> {
        let Some(path) = self.resolve_source_path(source, true)? else {
            return Ok(None);
        };
        let manifest_entries = load_manifest_entries(&path)?;
        if manifest_entries.is_empty() {
            return Ok(Some(ChecksumDigest {
                entries: 0,
                hash: Hasher::new().finalize().into(),
            }));
        }
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(manifest_entries.len());
        for (key, record) in manifest_entries {
            let encoded = serialize_contract_record(&record)?;
            pairs.push((key, encoded));
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut hasher = Hasher::new();
        for (key, value) in &pairs {
            hasher.update(b"market/contracts");
            hasher.update(&[0]);
            hasher.update(key.as_slice());
            hasher.update(&[0]);
            hasher.update(value.as_slice());
        }
        Ok(Some(ChecksumDigest {
            entries: pairs.len(),
            hash: hasher.finalize().into(),
        }))
    }

    pub fn database_checksum(&self, scope: ChecksumScope) -> Result<ChecksumDigest> {
        let mut entries: Vec<(String, Vec<u8>, Vec<u8>)> = match scope {
            ChecksumScope::ContractsOnly => {
                let mut items = Vec::new();
                for entry in self.contracts.iter() {
                    let (key, value) = entry?;
                    items.push(("market/contracts".to_string(), key, value));
                }
                items
            }
            ChecksumScope::AllColumnFamilies => self.collect_all_entries()?,
        };
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut hasher = Hasher::new();
        for (cf, key, value) in &entries {
            hasher.update(cf.as_bytes());
            hasher.update(&[0]);
            hasher.update(key);
            hasher.update(&[0]);
            hasher.update(value);
        }
        Ok(ChecksumDigest {
            entries: entries.len(),
            hash: hasher.finalize().into(),
        })
    }

    pub fn verify(
        &self,
        source: ManifestSource,
        scope: ChecksumScope,
    ) -> Result<ChecksumComparison> {
        let manifest = self.manifest_checksum(source)?;
        let database = self.database_checksum(scope)?;
        Ok(ChecksumComparison {
            scope,
            manifest,
            database,
        })
    }

    fn resolve_source_path(
        &self,
        source: ManifestSource,
        allow_absent: bool,
    ) -> Result<Option<PathBuf>> {
        let base = self.base_path();
        let path = match source {
            ManifestSource::Auto => match manifest_status(base) {
                ManifestStatus::Pending { path } => Some(path),
                ManifestStatus::Migrated { path } => Some(path),
                ManifestStatus::Absent => None,
            },
            ManifestSource::Pending => {
                let path = pending_manifest_path(base);
                if path.exists() {
                    Some(path)
                } else {
                    None
                }
            }
            ManifestSource::Migrated => {
                let path = migrated_manifest_path(base);
                if path.exists() {
                    Some(path)
                } else {
                    None
                }
            }
            ManifestSource::File(path) => {
                if path.exists() {
                    Some(path)
                } else {
                    return Err(StorageMarketError::LegacyManifest(format!(
                        "legacy manifest file {} does not exist",
                        display_path(&path)
                    )));
                }
            }
        };
        match path {
            Some(path) => Ok(Some(path)),
            None if allow_absent => Ok(None),
            None => Err(StorageMarketError::LegacyManifest(
                "legacy manifest not found".into(),
            )),
        }
    }

    fn scan_manifest(
        &self,
        source: ManifestSource,
        allow_absent: bool,
        sample_limit: Option<usize>,
    ) -> Result<ManifestScan> {
        let status = self.manifest_status();
        let Some(path) = self.resolve_source_path(source, allow_absent)? else {
            return Ok(ManifestScan {
                summary: ManifestSummary::empty(status),
                entries: Vec::new(),
                missing_keys: Vec::new(),
                duplicate_keys: Vec::new(),
            });
        };
        let entries = load_manifest_entries(&path)?;
        let mut present = 0usize;
        let mut missing = 0usize;
        let mut duplicates = 0usize;
        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        let sample_cap = sample_limit.unwrap_or(0);
        let mut statuses: Vec<AuditEntryStatus> = Vec::new();
        let mut missing_keys: Vec<Vec<u8>> = Vec::new();
        let mut duplicate_keys: Vec<Vec<u8>> = Vec::new();
        for (key, _) in &entries {
            let duplicate = !seen.insert(key.clone());
            if duplicate {
                duplicates = duplicates.saturating_add(1);
                if sample_cap > 0 && duplicate_keys.len() < sample_cap {
                    duplicate_keys.push(key.clone());
                }
            }
            let present_in_db = self.contracts.get(key.as_slice())?.is_some();
            if present_in_db {
                present = present.saturating_add(1);
            } else {
                missing = missing.saturating_add(1);
                if sample_cap > 0 && missing_keys.len() < sample_cap {
                    missing_keys.push(key.clone());
                }
            }
            if sample_cap > 0 && statuses.len() < sample_cap {
                statuses.push(AuditEntryStatus {
                    key: key.clone(),
                    present: present_in_db,
                    duplicate,
                });
            }
        }
        Ok(ManifestScan {
            summary: ManifestSummary {
                status,
                total_entries: entries.len(),
                present,
                missing,
                duplicates,
                source_path: Some(path),
            },
            entries: statuses,
            missing_keys,
            duplicate_keys,
        })
    }

    fn collect_all_entries(&self) -> Result<Vec<(String, Vec<u8>, Vec<u8>)>> {
        let mut cfs = self.engine.list_cfs()?;
        if !cfs.iter().any(|cf| cf == "default") {
            self.engine.ensure_cf("default")?;
            cfs.push("default".to_string());
        }
        cfs.sort();
        let mut entries = Vec::new();
        for cf in cfs {
            self.engine.ensure_cf(&cf)?;
            let tree = self.engine.open_tree(&cf)?;
            let mut cf_entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for item in tree.iter() {
                let (key, value) = item?;
                cf_entries.push((key, value));
            }
            cf_entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, value) in cf_entries {
                entries.push((cf.clone(), key, value));
            }
        }
        Ok(entries)
    }
}

fn apply_entries(
    tree: &Tree,
    entries: &[(Vec<u8>, ContractRecord)],
    mode: ImportMode,
) -> Result<ImportStats> {
    let mut stats = ImportStats::new(entries.len());
    for (key, record) in entries {
        let value = serialize_contract_record(record)?;
        match mode {
            ImportMode::InsertMissing => loop {
                if tree.get(key.as_slice())?.is_some() {
                    stats.skipped_existing = stats.skipped_existing.saturating_add(1);
                    break;
                }
                match tree.compare_and_swap(key.as_slice(), None, Some(value.clone()))? {
                    Ok(_) => {
                        stats.applied = stats.applied.saturating_add(1);
                        break;
                    }
                    Err(actual) => {
                        if actual.is_some() {
                            stats.skipped_existing = stats.skipped_existing.saturating_add(1);
                            break;
                        }
                    }
                }
            },
            ImportMode::OverwriteExisting => loop {
                let current = tree.get(key.as_slice())?;
                match current {
                    Some(ref bytes) if bytes == &value => {
                        stats.no_change = stats.no_change.saturating_add(1);
                        break;
                    }
                    Some(bytes) => match tree.compare_and_swap(
                        key.as_slice(),
                        Some(bytes.clone()),
                        Some(value.clone()),
                    )? {
                        Ok(_) => {
                            stats.overwritten = stats.overwritten.saturating_add(1);
                            break;
                        }
                        Err(actual) => match actual {
                            Some(actual_bytes) if actual_bytes == value => {
                                stats.no_change = stats.no_change.saturating_add(1);
                                break;
                            }
                            Some(_) => continue,
                            None => continue,
                        },
                    },
                    None => {
                        match tree.compare_and_swap(key.as_slice(), None, Some(value.clone()))? {
                            Ok(_) => {
                                stats.applied = stats.applied.saturating_add(1);
                                break;
                            }
                            Err(actual) => match actual {
                                Some(actual_bytes) if actual_bytes == value => {
                                    stats.no_change = stats.no_change.saturating_add(1);
                                    break;
                                }
                                Some(_) => continue,
                                None => continue,
                            },
                        }
                    }
                }
            },
        }
    }
    Ok(stats)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

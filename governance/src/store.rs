use super::{
    registry, ApprovedRelease, ParamKey, Params, Proposal, ProposalStatus, ReleaseBallot,
    ReleaseVote, Runtime, Vote, VoteChoice,
};
use crate::params::{
    decode_runtime_backend_policy, decode_storage_engine_policy, decode_transport_provider_policy,
};
use crate::treasury::{mark_cancelled, mark_executed, TreasuryDisbursement};
use foundation_lazy::sync::Lazy;
use foundation_serialization::de::DeserializeOwned;
use foundation_serialization::{binary, json, Deserialize, Serialize};
use sled::Config;
use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ACTIVATION_DELAY: u64 = 2;
pub const ROLLBACK_WINDOW_EPOCHS: u64 = 1;
pub const QUORUM: u64 = 1;
const PARAM_HISTORY_LIMIT: usize = 512;
const DID_REVOCATION_HISTORY_LIMIT: usize = 512;
const TREASURY_HISTORY_LIMIT: usize = 1024;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "foundation_serialization::serde")]
pub struct LastActivation {
    pub proposal_id: u64,
    pub key: ParamKey,
    pub old_value: i64,
    pub new_value: i64,
    pub activated_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct ParamChangeRecord {
    key: ParamKey,
    proposal_id: u64,
    epoch: u64,
    old_value: i64,
    new_value: i64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    fee_floor: Option<FeeFloorPolicySnapshot>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    dependency_policy: Option<DependencyPolicySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct FeeFloorPolicySnapshot {
    window: i64,
    percentile: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct FeeFloorPolicyRecord {
    epoch: u64,
    proposal_id: u64,
    window: i64,
    percentile: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DependencyPolicySnapshot {
    kind: String,
    allowed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DependencyPolicyRecord {
    pub epoch: u64,
    pub proposal_id: u64,
    pub kind: String,
    pub allowed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DidRevocationRecord {
    pub address: String,
    pub reason: String,
    pub epoch: u64,
    pub revoked_at: u64,
}

#[derive(Clone)]
pub struct GovStore {
    db: Arc<sled::Db>,
    base_path: PathBuf,
}

static GOV_DB_REGISTRY: Lazy<Mutex<HashMap<PathBuf, Weak<sled::Db>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn ser<T: Serialize>(value: &T) -> sled::Result<Vec<u8>> {
    binary::encode(value).map_err(|e| sled::Error::Unsupported(format!("ser: {e}").into()))
}

fn de<T: DeserializeOwned>(bytes: &[u8]) -> sled::Result<T> {
    binary::decode(bytes).map_err(|e| sled::Error::Unsupported(format!("de: {e}").into()))
}

fn decode_install_times(bytes: &[u8]) -> sled::Result<Vec<u64>> {
    match de::<Vec<u64>>(bytes) {
        Ok(list) => Ok(list),
        Err(_) => de::<u64>(bytes).map(|single| vec![single]),
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn derive_base_path(path: &Path) -> PathBuf {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.is_dir() {
            if path.extension().is_some() {
                return path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."));
            }
            return path.to_path_buf();
        }
    }
    if path.extension().is_none() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

impl GovStore {
    fn did_revocations(&self) -> sled::Tree {
        self.db
            .open_tree("did_revocations")
            .unwrap_or_else(|e| panic!("open did revocation tree: {e}"))
    }

    fn persist_did_revocation(&self, record: &DidRevocationRecord) {
        let hist_dir = self.base_path.join("governance/history");
        let _ = std::fs::create_dir_all(&hist_dir);
        let path = hist_dir.join("did_revocations.json");
        let mut history: Vec<DidRevocationRecord> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| json::from_slice(&bytes).ok())
            .unwrap_or_default();
        history.push(record.clone());
        if history.len() > DID_REVOCATION_HISTORY_LIMIT {
            history.drain(0..history.len() - DID_REVOCATION_HISTORY_LIMIT);
        }
        if let Ok(bytes) = json::to_vec(&history) {
            let _ = std::fs::write(&path, bytes);
        }
    }

    fn treasury_disbursement_path(&self) -> PathBuf {
        self.base_path
            .join("governance")
            .join("treasury_disbursements.json")
    }

    fn load_disbursements(&self) -> sled::Result<Vec<TreasuryDisbursement>> {
        let path = self.treasury_disbursement_path();
        match std::fs::read(&path) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    Ok(Vec::new())
                } else {
                    json::from_slice(&bytes).map_err(|e| {
                        sled::Error::Unsupported(
                            format!("decode treasury disbursements: {e}").into(),
                        )
                    })
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(sled::Error::Unsupported(
                format!("read treasury disbursements: {err}").into(),
            )),
        }
    }

    fn persist_disbursements(&self, records: &[TreasuryDisbursement]) -> sled::Result<()> {
        let mut trimmed = records.to_vec();
        trimmed.sort_by_key(|record| record.id);
        if trimmed.len() > TREASURY_HISTORY_LIMIT {
            let drop = trimmed.len() - TREASURY_HISTORY_LIMIT;
            trimmed.drain(0..drop);
        }
        let bytes = json::to_vec(&trimmed).map_err(|e| {
            sled::Error::Unsupported(format!("encode treasury disbursements: {e}").into())
        })?;
        let path = self.treasury_disbursement_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, bytes).map_err(|e| {
            sled::Error::Unsupported(format!("write treasury disbursements: {e}").into())
        })
    }

    fn persist_fee_floor_policy(
        &self,
        hist_dir: &Path,
        epoch: u64,
        proposal_id: u64,
        snapshot: FeeFloorPolicySnapshot,
    ) {
        let path = hist_dir.join("fee_floor_policy.json");
        let mut history: Vec<FeeFloorPolicyRecord> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| json::from_slice(&bytes).ok())
            .unwrap_or_default();
        history.push(FeeFloorPolicyRecord {
            epoch,
            proposal_id,
            window: snapshot.window,
            percentile: snapshot.percentile,
        });
        if history.len() > PARAM_HISTORY_LIMIT {
            history.drain(0..history.len() - PARAM_HISTORY_LIMIT);
        }
        if let Ok(bytes) = json::to_vec(&history) {
            let _ = std::fs::write(&path, bytes);
        }
    }

    fn persist_dependency_policy(
        &self,
        hist_dir: &Path,
        epoch: u64,
        proposal_id: u64,
        snapshot: &DependencyPolicySnapshot,
    ) {
        let path = hist_dir.join("dependency_policy.json");
        let mut history: Vec<DependencyPolicyRecord> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| json::from_slice(&bytes).ok())
            .unwrap_or_default();
        history.push(DependencyPolicyRecord {
            epoch,
            proposal_id,
            kind: snapshot.kind.clone(),
            allowed: snapshot.allowed.clone(),
        });
        if history.len() > PARAM_HISTORY_LIMIT {
            history.drain(0..history.len() - PARAM_HISTORY_LIMIT);
        }
        if let Ok(bytes) = json::to_vec(&history) {
            let _ = std::fs::write(&path, bytes);
        }
    }

    fn persist_param_change(
        &self,
        hist_dir: &Path,
        key: ParamKey,
        proposal_id: u64,
        old_value: i64,
        new_value: i64,
        epoch: u64,
        params: &Params,
    ) {
        let fee_snapshot = if matches!(key, ParamKey::FeeFloorWindow | ParamKey::FeeFloorPercentile)
        {
            Some(FeeFloorPolicySnapshot {
                window: params.fee_floor_window,
                percentile: params.fee_floor_percentile,
            })
        } else {
            None
        };

        let dependency_snapshot = match key {
            ParamKey::RuntimeBackend => Some(DependencyPolicySnapshot {
                kind: "runtime_backend".to_string(),
                allowed: decode_runtime_backend_policy(params.runtime_backend_policy),
            }),
            ParamKey::TransportProvider => Some(DependencyPolicySnapshot {
                kind: "transport_provider".to_string(),
                allowed: decode_transport_provider_policy(params.transport_provider_policy),
            }),
            ParamKey::StorageEnginePolicy => Some(DependencyPolicySnapshot {
                kind: "storage_engine".to_string(),
                allowed: decode_storage_engine_policy(params.storage_engine_policy),
            }),
            _ => None,
        };

        let record = ParamChangeRecord {
            key,
            proposal_id,
            epoch,
            old_value,
            new_value,
            fee_floor: fee_snapshot.clone(),
            dependency_policy: dependency_snapshot.clone(),
        };

        let path = hist_dir.join("param_changes.json");
        let mut history: Vec<ParamChangeRecord> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| json::from_slice(&bytes).ok())
            .unwrap_or_default();
        history.push(record);
        if history.len() > PARAM_HISTORY_LIMIT {
            history.drain(0..history.len() - PARAM_HISTORY_LIMIT);
        }
        if let Ok(bytes) = json::to_vec(&history) {
            let _ = std::fs::write(&path, bytes);
        }

        if let Some(snapshot) = fee_snapshot {
            self.persist_fee_floor_policy(hist_dir, epoch, proposal_id, snapshot);
        }

        if let Some(snapshot) = dependency_snapshot {
            self.persist_dependency_policy(hist_dir, epoch, proposal_id, &snapshot);
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Self {
        let normalized = normalize_path(path.as_ref());
        let mut registry = GOV_DB_REGISTRY.lock().unwrap();
        if let Some(existing) = registry.get(&normalized) {
            if let Some(db) = existing.upgrade() {
                let base = derive_base_path(&normalized);
                drop(registry);
                return Self {
                    db,
                    base_path: base,
                };
            }
        }
        registry.remove(&normalized);
        let db_handle = Config::new()
            .path(&normalized)
            .open()
            .unwrap_or_else(|e| panic!("open db: {e}"));
        let db = Arc::new(db_handle);
        registry.insert(normalized.clone(), Arc::downgrade(&db));
        drop(registry);
        let base = derive_base_path(&normalized);
        Self {
            db,
            base_path: base,
        }
    }

    /// Record a DID revocation enforced by governance.
    pub fn revoke_did(&self, address: &str, reason: &str, epoch: u64) -> sled::Result<()> {
        let mut rec = DidRevocationRecord {
            address: address.to_string(),
            reason: reason.to_string(),
            epoch,
            revoked_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let bytes = ser(&rec)?;
        self.did_revocations().insert(address.as_bytes(), bytes)?;
        self.persist_did_revocation(&rec);
        rec.reason.shrink_to_fit();
        Ok(())
    }

    /// Clear a previously recorded DID revocation.
    pub fn clear_did_revocation(&self, address: &str) -> sled::Result<()> {
        self.did_revocations().remove(address.as_bytes())?;
        Ok(())
    }

    /// Determine whether a DID is currently revoked.
    pub fn is_did_revoked(&self, address: &str) -> bool {
        self.did_revocations()
            .get(address.as_bytes())
            .ok()
            .flatten()
            .is_some()
    }

    /// Retrieve recorded DID revocation history for monitoring and explorer use.
    pub fn did_revocation_history(&self) -> sled::Result<Vec<DidRevocationRecord>> {
        let hist_dir = self.base_path.join("governance/history");
        let path = hist_dir.join("did_revocations.json");
        if let Ok(bytes) = std::fs::read(&path) {
            json::from_slice(&bytes).map_err(|e| {
                sled::Error::Unsupported(format!("de did revocation history: {e}").into())
            })
        } else {
            Ok(Vec::new())
        }
    }

    pub fn dependency_policy_history(&self) -> sled::Result<Vec<DependencyPolicyRecord>> {
        let hist_dir = self.base_path.join("governance/history");
        let path = hist_dir.join("dependency_policy.json");
        if let Ok(bytes) = std::fs::read(&path) {
            json::from_slice(&bytes).map_err(|e| {
                sled::Error::Unsupported(format!("de dependency policy history: {e}").into())
            })
        } else {
            Ok(Vec::new())
        }
    }

    pub fn proposals(&self) -> sled::Tree {
        self.db
            .open_tree("proposals")
            .unwrap_or_else(|e| panic!("open proposals tree: {e}"))
    }
    fn votes(&self, id: u64) -> sled::Tree {
        self.db
            .open_tree(format!("votes/{id}"))
            .unwrap_or_else(|e| panic!("open votes tree: {e}"))
    }
    fn next_id(&self) -> sled::Tree {
        self.db
            .open_tree("next_id")
            .unwrap_or_else(|e| panic!("open next_id tree: {e}"))
    }
    fn active_params(&self) -> sled::Tree {
        self.db
            .open_tree("active_params")
            .unwrap_or_else(|e| panic!("open active_params tree: {e}"))
    }
    fn activation_queue(&self) -> sled::Tree {
        self.db
            .open_tree("activation_queue")
            .unwrap_or_else(|e| panic!("open activation_queue tree: {e}"))
    }
    fn last_activation(&self) -> sled::Tree {
        self.db
            .open_tree("last_activation")
            .unwrap_or_else(|e| panic!("open last_activation tree: {e}"))
    }

    fn release_proposals(&self) -> sled::Tree {
        self.db
            .open_tree("release_proposals")
            .unwrap_or_else(|e| panic!("open release_proposals tree: {e}"))
    }

    fn release_votes(&self, id: u64) -> sled::Tree {
        self.db
            .open_tree(format!("release_votes/{id}"))
            .unwrap_or_else(|e| panic!("open release_votes tree: {e}"))
    }

    fn release_next_id(&self) -> sled::Tree {
        self.db
            .open_tree("release_next_id")
            .unwrap_or_else(|e| panic!("open release_next_id tree: {e}"))
    }

    fn approved_releases(&self) -> sled::Tree {
        self.db
            .open_tree("approved_releases")
            .unwrap_or_else(|e| panic!("open approved_releases tree: {e}"))
    }

    fn release_installs(&self) -> sled::Tree {
        self.db
            .open_tree("release_installs")
            .unwrap_or_else(|e| panic!("open release_installs tree: {e}"))
    }

    pub fn submit(&self, mut p: Proposal) -> sled::Result<u64> {
        if p.new_value < p.min || p.new_value > p.max {
            return Err(sled::Error::Unsupported("out of bounds".into()));
        }
        // Ensure dependencies exist and graph remains acyclic
        for dep in &p.deps {
            if self.proposals().get(ser(dep)?)?.is_none() {
                return Err(sled::Error::Unsupported("missing dependency".into()));
            }
        }
        let next = self
            .next_id()
            .get("id")?
            .map(|v| de::<u64>(&v))
            .transpose()?
            .unwrap_or(0);
        self.next_id().insert("id", ser(&(next + 1))?)?;
        p.id = next;
        // collect existing proposals for cycle detection
        let mut existing = std::collections::HashMap::new();
        for item in self.proposals().iter() {
            let (k, v) = item?;
            let id: u64 = de(&k)?;
            let prop: Proposal = de(&v)?;
            existing.insert(id, prop);
        }
        if !super::validate_dag(&existing, &p) {
            return Err(sled::Error::Unsupported("cycle".into()));
        }
        self.proposals().insert(ser(&p.id)?, ser(&p)?)?;
        Ok(next)
    }

    pub fn submit_release(&self, mut r: ReleaseVote) -> sled::Result<u64> {
        if r.build_hash.len() != 64 || !r.build_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(sled::Error::Unsupported("invalid release hash".into()));
        }
        if self.is_release_hash_known(&r.build_hash)? {
            return Err(sled::Error::Unsupported(
                "release hash already known".into(),
            ));
        }
        if !r.signer_set.is_empty() && r.signature_threshold as usize > r.signer_set.len() {
            return Err(sled::Error::Unsupported(
                "threshold exceeds signer set".into(),
            ));
        }
        let next = self
            .release_next_id()
            .get("id")?
            .map(|v| de::<u64>(&v))
            .transpose()?
            .unwrap_or(0);
        self.release_next_id().insert("id", ser(&(next + 1))?)?;
        r.id = next;
        self.release_proposals().insert(ser(&r.id)?, ser(&r)?)?;
        Ok(next)
    }

    fn is_release_hash_known(&self, hash: &str) -> sled::Result<bool> {
        if self.approved_releases().get(hash.as_bytes())?.is_some() {
            return Ok(true);
        }
        for item in self.release_proposals().iter() {
            let (_, v) = item?;
            let prop: ReleaseVote = de(&v)?;
            if prop.build_hash == hash {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn vote(&self, proposal_id: u64, mut v: Vote, current_epoch: u64) -> sled::Result<()> {
        let prop_raw = self
            .proposals()
            .get(ser(&proposal_id)?)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?;
        let prop: Proposal = de(&prop_raw)?;
        if current_epoch >= prop.vote_deadline_epoch {
            return Err(sled::Error::Unsupported("deadline".into()));
        }
        v.received_at = current_epoch;
        self.votes(proposal_id)
            .insert(v.voter.as_bytes(), ser(&v)?)?;
        Ok(())
    }

    pub fn vote_release(&self, proposal_id: u64, mut v: ReleaseBallot) -> sled::Result<()> {
        let prop_key = ser(&proposal_id)?;
        let prop_raw = self
            .release_proposals()
            .get(&prop_key)?
            .ok_or_else(|| sled::Error::Unsupported("missing release proposal".into()))?;
        let prop: ReleaseVote = de(&prop_raw)?;
        if v.received_at > prop.vote_deadline_epoch {
            return Err(sled::Error::Unsupported("deadline".into()));
        }
        v.proposal_id = proposal_id;
        self.release_votes(proposal_id)
            .insert(v.voter.as_bytes(), ser(&v)?)?;
        Ok(())
    }

    pub fn tally_and_queue(
        &self,
        proposal_id: u64,
        current_epoch: u64,
    ) -> sled::Result<ProposalStatus> {
        let key = ser(&proposal_id)?;
        let mut prop: Proposal = de(&self
            .proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?)?;
        if prop.status != ProposalStatus::Open {
            return Ok(prop.status);
        }
        if current_epoch < prop.vote_deadline_epoch {
            return Ok(ProposalStatus::Open);
        }
        let votes = self.votes(proposal_id);
        let mut yes = 0u64;
        let mut no = 0u64;
        for v in votes.iter() {
            let (_, raw) = v?;
            let vote: Vote = de(&raw)?;
            match vote.choice {
                VoteChoice::Yes => yes += vote.weight,
                VoteChoice::No => no += vote.weight,
                _ => {}
            }
        }
        if yes >= QUORUM && yes > no {
            prop.status = ProposalStatus::Passed;
            let spec = registry()
                .iter()
                .find(|s| s.key == prop.key)
                .expect("param spec");
            let delay = if spec.timelock_epochs > 0 {
                spec.timelock_epochs
            } else {
                ACTIVATION_DELAY
            };
            let act_epoch = current_epoch + delay;
            prop.activation_epoch = Some(act_epoch);
            let key_epoch = ser(&act_epoch)?;
            let mut list: Vec<u64> = self
                .activation_queue()
                .get(&key_epoch)?
                .map(|v| de(&v))
                .transpose()?
                .unwrap_or_else(|| vec![]);
            list.push(proposal_id);
            self.activation_queue().insert(key_epoch, ser(&list)?)?;
        } else {
            prop.status = ProposalStatus::Rejected;
        }
        self.proposals().insert(&key, ser(&prop)?)?;
        Ok(prop.status)
    }

    pub fn tally_release(
        &self,
        proposal_id: u64,
        current_epoch: u64,
    ) -> sled::Result<ProposalStatus> {
        let key = ser(&proposal_id)?;
        let mut prop: ReleaseVote = de(&self
            .release_proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing release proposal".into()))?)?;
        if !prop.is_open() {
            return Ok(prop.status);
        }
        if current_epoch < prop.vote_deadline_epoch {
            return Ok(prop.status);
        }
        let mut yes = 0u64;
        let mut no = 0u64;
        for item in self.release_votes(proposal_id).iter() {
            let (_, raw) = item?;
            let vote: ReleaseBallot = de(&raw)?;
            match vote.choice {
                VoteChoice::Yes => yes += vote.weight,
                VoteChoice::No => no += vote.weight,
                VoteChoice::Abstain => {}
            }
        }
        if ReleaseVote::quorum_met(yes) && yes >= no {
            prop.mark_passed(current_epoch);
            prop.mark_activated(current_epoch);
            self.release_proposals().insert(&key, ser(&prop)?)?;
            let installs: Vec<u64> = self
                .release_installs()
                .get(prop.build_hash.as_bytes())?
                .map(|raw| decode_install_times(&raw))
                .transpose()?
                .unwrap_or_default();
            let record = ApprovedRelease {
                build_hash: prop.build_hash.clone(),
                activated_epoch: current_epoch,
                proposer: prop.proposer.clone(),
                signatures: prop.signatures.clone(),
                signature_threshold: prop.signature_threshold,
                signer_set: prop.signer_set.clone(),
                install_times: installs,
            };
            self.approved_releases()
                .insert(prop.build_hash.as_bytes(), ser(&record)?)?;
            Ok(ProposalStatus::Activated)
        } else if ReleaseVote::quorum_met(no) && no > yes {
            prop.mark_rejected();
            self.release_proposals().insert(&key, ser(&prop)?)?;
            Ok(ProposalStatus::Rejected)
        } else {
            Ok(prop.status)
        }
    }

    pub fn approved_release_hashes(&self) -> sled::Result<Vec<ApprovedRelease>> {
        let mut installs: std::collections::HashMap<String, Vec<u64>> =
            std::collections::HashMap::new();
        for item in self.release_installs().iter() {
            let (hash_bytes, ts_bytes) = item?;
            let hash = String::from_utf8(hash_bytes.to_vec())
                .map_err(|e| sled::Error::Unsupported(format!("utf8: {e}").into()))?;
            let times: Vec<u64> = decode_install_times(&ts_bytes)?;
            installs.insert(hash, times);
        }
        let mut out = Vec::new();
        for item in self.approved_releases().iter() {
            let (_, raw) = item?;
            let mut record: ApprovedRelease = de(&raw)?;
            if let Some(times) = installs.get(&record.build_hash) {
                record.install_times = times.clone();
            }
            out.push(record);
        }
        Ok(out)
    }

    pub fn is_release_hash_approved(&self, hash: &str) -> sled::Result<bool> {
        Ok(self.approved_releases().get(hash.as_bytes())?.is_some())
    }

    pub fn record_release_install(&self, hash: &str) -> sled::Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut installs: Vec<u64> = self
            .release_installs()
            .get(hash.as_bytes())?
            .map(|raw| decode_install_times(&raw))
            .transpose()?
            .unwrap_or_default();
        installs.push(now);
        installs.sort_unstable();
        self.release_installs()
            .insert(hash.as_bytes(), ser(&installs)?)?;
        if let Some(existing) = self
            .approved_releases()
            .get(hash.as_bytes())?
            .map(|raw| de::<ApprovedRelease>(&raw))
            .transpose()?
        {
            let mut updated = existing;
            updated.install_times = installs.clone();
            self.approved_releases()
                .insert(hash.as_bytes(), ser(&updated)?)?;
        }
        Ok(())
    }

    pub fn release_installations(&self) -> sled::Result<Vec<(String, Vec<u64>)>> {
        let mut installs = Vec::new();
        for item in self.release_installs().iter() {
            let (hash_bytes, ts_bytes) = item?;
            let hash = String::from_utf8(hash_bytes.to_vec())
                .map_err(|e| sled::Error::Unsupported(format!("utf8: {e}").into()))?;
            let ts: Vec<u64> = decode_install_times(&ts_bytes)?;
            installs.push((hash, ts));
        }
        Ok(installs)
    }

    pub fn activate_ready(
        &self,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        rt.set_current_params(params);
        // snapshot current params before applying any changes
        let hist_dir = self.base_path.join("governance/history");
        let _ = std::fs::create_dir_all(&hist_dir);
        let snap_path = hist_dir.join(format!("{}.json", current_epoch));
        if let Ok(bytes) = json::to_vec(params) {
            let _ = std::fs::write(&snap_path, bytes);
        }

        let queue = self.activation_queue();
        let mut to_remove = vec![];
        for item in queue.iter() {
            let (k, v) = item?;
            let epoch: u64 = de(&k)?;
            if epoch <= current_epoch {
                let ids: Vec<u64> = de(&v).unwrap_or_else(|_| vec![]);
                for prop_id in ids {
                    let key = ser(&prop_id)?;
                    if let Some(raw) = self.proposals().get(&key)? {
                        let mut prop: Proposal = de(&raw)?;
                        if prop.status == ProposalStatus::Passed {
                            let old = match prop.key {
                                ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
                                ParamKey::ConsumerFeeComfortP90Microunits => {
                                    params.consumer_fee_comfort_p90_microunits
                                }
                                ParamKey::IndustrialAdmissionMinCapacity => {
                                    params.industrial_admission_min_capacity
                                }
                                ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
                                ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
                                ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
                                ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
                                ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
                                ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
                                ParamKey::ProofRebateLimitCt => params.proof_rebate_limit_ct,
                                ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
                                ParamKey::KillSwitchSubsidyReduction => {
                                    params.kill_switch_subsidy_reduction
                                }
                                ParamKey::MinerRewardLogisticTarget => {
                                    params.miner_reward_logistic_target
                                }
                                ParamKey::LogisticSlope => params.logistic_slope_milli,
                                ParamKey::MinerHysteresis => params.miner_hysteresis,
                                ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
                                ParamKey::FeeFloorWindow => params.fee_floor_window,
                                ParamKey::FeeFloorPercentile => params.fee_floor_percentile,
                                ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
                                ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
                                ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
                                ParamKey::JurisdictionRegion => params.jurisdiction_region,
                                ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
                                ParamKey::KalmanRShort => params.kalman_r_short,
                                ParamKey::KalmanRMed => params.kalman_r_med,
                                ParamKey::KalmanRLong => params.kalman_r_long,
                                ParamKey::SchedulerWeightGossip => params.scheduler_weight_gossip,
                                ParamKey::SchedulerWeightCompute => params.scheduler_weight_compute,
                                ParamKey::SchedulerWeightStorage => params.scheduler_weight_storage,
                                ParamKey::RuntimeBackend => params.runtime_backend_policy,
                                ParamKey::TransportProvider => params.transport_provider_policy,
                                ParamKey::StorageEnginePolicy => params.storage_engine_policy,
                            };
                            if let Some(spec) = registry().iter().find(|s| s.key == prop.key) {
                                (spec.apply)(prop.new_value, params)
                                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                                (spec.apply_runtime)(prop.new_value, rt)
                                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                            }
                            let last = LastActivation {
                                proposal_id: prop.id,
                                key: prop.key,
                                old_value: old,
                                new_value: prop.new_value,
                                activated_epoch: current_epoch,
                            };
                            self.last_activation().insert("last", ser(&last)?)?;
                            prop.status = ProposalStatus::Activated;
                            self.proposals().insert(&key, ser(&prop)?)?;
                            self.active_params()
                                .insert(ser(&prop.key)?, ser(&prop.new_value)?)?;
                            self.persist_param_change(
                                &hist_dir,
                                prop.key,
                                prop.id,
                                old,
                                prop.new_value,
                                current_epoch,
                                params,
                            );
                        }
                    }
                }
                to_remove.push(epoch);
            }
        }
        for e in to_remove {
            queue.remove(ser(&e)?)?;
        }
        rt.clear_current_params();
        Ok(())
    }

    pub fn last_activation_record(&self) -> sled::Result<Option<LastActivation>> {
        match self.last_activation().get("last")? {
            Some(raw) => de(&raw).map(Some),
            None => Ok(None),
        }
    }

    pub fn rollback_last(
        &self,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        if let Some(raw) = self.last_activation().get("last")? {
            rt.set_current_params(params);
            let hist_dir = self.base_path.join("governance/history");
            let _ = std::fs::create_dir_all(&hist_dir);
            let last: LastActivation = de(&raw)?;
            if current_epoch > last.activated_epoch + ROLLBACK_WINDOW_EPOCHS {
                return Err(sled::Error::Unsupported("expired".into()));
            }
            if let Some(spec) = registry().iter().find(|s| s.key == last.key) {
                (spec.apply)(last.old_value, params)
                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                (spec.apply_runtime)(last.old_value, rt)
                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
            }
            self.active_params()
                .insert(ser(&last.key)?, ser(&last.old_value)?)?;
            self.persist_param_change(
                &hist_dir,
                last.key,
                last.proposal_id,
                last.new_value,
                last.old_value,
                current_epoch,
                params,
            );
            if let Some(prop_raw) = self.proposals().get(ser(&last.proposal_id)?)? {
                let mut prop: Proposal = de(&prop_raw)?;
                prop.status = ProposalStatus::RolledBack;
                self.proposals().insert(ser(&prop.id)?, ser(&prop)?)?;
            }
            self.last_activation().remove("last")?;
            rt.clear_current_params();
            return Ok(());
        }
        Err(sled::Error::ReportableBug("no activation".into()))
    }

    pub fn rollback_proposal(
        &self,
        proposal_id: u64,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        let key = ser(&proposal_id)?;
        let prop_raw = self
            .proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?;
        let mut prop: Proposal = de(&prop_raw)?;
        let act_epoch = prop
            .activation_epoch
            .ok_or_else(|| sled::Error::Unsupported("not activated".into()))?;
        if current_epoch > act_epoch + ROLLBACK_WINDOW_EPOCHS {
            return Err(sled::Error::Unsupported("expired".into()));
        }
        let snap_path = self
            .base_path
            .join("governance/history")
            .join(format!("{}.json", act_epoch));
        let hist_dir = self.base_path.join("governance/history");
        let _ = std::fs::create_dir_all(&hist_dir);
        let bytes =
            std::fs::read(&snap_path).map_err(|_| sled::Error::Unsupported("snapshot".into()))?;
        let prev: Params =
            json::from_slice(&bytes).map_err(|_| sled::Error::Unsupported("parse".into()))?;
        *params = prev.clone();
        rt.set_current_params(params);
        for spec in registry() {
            let val = match spec.key {
                ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
                ParamKey::ConsumerFeeComfortP90Microunits => {
                    params.consumer_fee_comfort_p90_microunits
                }
                ParamKey::IndustrialAdmissionMinCapacity => {
                    params.industrial_admission_min_capacity
                }
                ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
                ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
                ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
                ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
                ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
                ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
                ParamKey::ProofRebateLimitCt => params.proof_rebate_limit_ct,
                ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
                ParamKey::KillSwitchSubsidyReduction => params.kill_switch_subsidy_reduction as i64,
                ParamKey::MinerRewardLogisticTarget => params.miner_reward_logistic_target,
                ParamKey::LogisticSlope => params.logistic_slope_milli,
                ParamKey::MinerHysteresis => params.miner_hysteresis,
                ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
                ParamKey::FeeFloorWindow => params.fee_floor_window,
                ParamKey::FeeFloorPercentile => params.fee_floor_percentile,
                ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
                ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
                ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
                ParamKey::JurisdictionRegion => params.jurisdiction_region,
                ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
                ParamKey::KalmanRShort => params.kalman_r_short,
                ParamKey::KalmanRMed => params.kalman_r_med,
                ParamKey::KalmanRLong => params.kalman_r_long,
                ParamKey::SchedulerWeightGossip => params.scheduler_weight_gossip,
                ParamKey::SchedulerWeightCompute => params.scheduler_weight_compute,
                ParamKey::SchedulerWeightStorage => params.scheduler_weight_storage,
                ParamKey::RuntimeBackend => params.runtime_backend_policy,
                ParamKey::TransportProvider => params.transport_provider_policy,
                ParamKey::StorageEnginePolicy => params.storage_engine_policy,
            };
            (spec.apply_runtime)(val, rt)
                .map_err(|_| sled::Error::Unsupported("apply_runtime".into()))?;
            self.active_params().insert(ser(&spec.key)?, ser(&val)?)?;
        }
        let reverted_val = match prop.key {
            ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
            ParamKey::ConsumerFeeComfortP90Microunits => params.consumer_fee_comfort_p90_microunits,
            ParamKey::IndustrialAdmissionMinCapacity => params.industrial_admission_min_capacity,
            ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
            ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
            ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
            ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
            ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
            ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
            ParamKey::ProofRebateLimitCt => params.proof_rebate_limit_ct,
            ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
            ParamKey::KillSwitchSubsidyReduction => params.kill_switch_subsidy_reduction as i64,
            ParamKey::MinerRewardLogisticTarget => params.miner_reward_logistic_target,
            ParamKey::LogisticSlope => params.logistic_slope_milli,
            ParamKey::MinerHysteresis => params.miner_hysteresis,
            ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
            ParamKey::FeeFloorWindow => params.fee_floor_window,
            ParamKey::FeeFloorPercentile => params.fee_floor_percentile,
            ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
            ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
            ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
            ParamKey::JurisdictionRegion => params.jurisdiction_region,
            ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
            ParamKey::KalmanRShort => params.kalman_r_short,
            ParamKey::KalmanRMed => params.kalman_r_med,
            ParamKey::KalmanRLong => params.kalman_r_long,
            ParamKey::SchedulerWeightGossip => params.scheduler_weight_gossip,
            ParamKey::SchedulerWeightCompute => params.scheduler_weight_compute,
            ParamKey::SchedulerWeightStorage => params.scheduler_weight_storage,
            ParamKey::RuntimeBackend => params.runtime_backend_policy,
            ParamKey::TransportProvider => params.transport_provider_policy,
            ParamKey::StorageEnginePolicy => params.storage_engine_policy,
        };
        self.persist_param_change(
            &hist_dir,
            prop.key,
            prop.id,
            prop.new_value,
            reverted_val,
            current_epoch,
            params,
        );
        prop.status = ProposalStatus::RolledBack;
        self.proposals().insert(key, ser(&prop)?)?;
        rt.clear_current_params();
        Ok(())
    }

    pub fn queue_disbursement(
        &self,
        destination: &str,
        amount_ct: u64,
        memo: &str,
        scheduled_epoch: u64,
    ) -> sled::Result<TreasuryDisbursement> {
        let mut records = self.load_disbursements()?;
        let next_id = records
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let record = TreasuryDisbursement::new(
            next_id,
            destination.to_string(),
            amount_ct,
            memo.to_string(),
            scheduled_epoch,
        );
        records.push(record.clone());
        self.persist_disbursements(&records)?;
        Ok(record)
    }

    pub fn disbursements(&self) -> sled::Result<Vec<TreasuryDisbursement>> {
        self.load_disbursements()
    }

    pub fn execute_disbursement(
        &self,
        id: u64,
        tx_hash: &str,
    ) -> sled::Result<TreasuryDisbursement> {
        let mut records = self.load_disbursements()?;
        let mut record = None;
        for entry in records.iter_mut() {
            if entry.id == id {
                mark_executed(entry, tx_hash.to_string());
                record = Some(entry.clone());
                break;
            }
        }
        if let Some(updated) = record.clone() {
            self.persist_disbursements(&records)?;
            Ok(updated)
        } else {
            Err(sled::Error::Unsupported(
                format!("unknown treasury disbursement id {id}").into(),
            ))
        }
    }

    pub fn cancel_disbursement(&self, id: u64, reason: &str) -> sled::Result<TreasuryDisbursement> {
        let mut records = self.load_disbursements()?;
        let mut record = None;
        for entry in records.iter_mut() {
            if entry.id == id {
                mark_cancelled(entry, reason.to_string());
                record = Some(entry.clone());
                break;
            }
        }
        if let Some(updated) = record.clone() {
            self.persist_disbursements(&records)?;
            Ok(updated)
        } else {
            Err(sled::Error::Unsupported(
                format!("unknown treasury disbursement id {id}").into(),
            ))
        }
    }
}

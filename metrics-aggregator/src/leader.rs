use crate::{AppState, LeaderSnapshot};
use diagnostics::tracing::{info, warn};
use foundation_serialization::{json, Deserialize, Error as SerializationError, Serialize};
use runtime::sleep;
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use storage_engine::{inhouse_engine::InhouseEngine, KeyValue};

const LEADER_CF: &str = "coordination";
const LEADER_KEY: &[u8] = b"leader";
const MIN_SLEEP: Duration = Duration::from_millis(250);
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(15);
const DEFAULT_RENEW_MARGIN: Duration = Duration::from_secs(5);
const DEFAULT_RETRY_BACKOFF: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderElectionConfig {
    pub lease_ttl: Duration,
    pub renew_margin: Duration,
    pub retry_backoff: Duration,
    pub instance_id: String,
}

impl LeaderElectionConfig {
    pub fn new(
        instance_id: impl Into<String>,
        lease_ttl: Duration,
        renew_margin: Duration,
    ) -> Self {
        Self {
            instance_id: instance_id.into(),
            lease_ttl,
            renew_margin,
            retry_backoff: DEFAULT_RETRY_BACKOFF,
        }
    }

    pub fn from_env() -> Self {
        let lease_ttl = env::var("AGGREGATOR_LEASE_TTL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_millis)
            .or_else(|| {
                env::var("AGGREGATOR_LEASE_TTL_SECS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
            })
            .unwrap_or(DEFAULT_LEASE_TTL);

        let renew_margin = env::var("AGGREGATOR_LEASE_RENEW_MARGIN_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_millis)
            .or_else(|| {
                env::var("AGGREGATOR_LEASE_RENEW_MARGIN_SECS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
            })
            .unwrap_or(DEFAULT_RENEW_MARGIN);

        let retry_backoff = env::var("AGGREGATOR_LEASE_RETRY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_RETRY_BACKOFF);

        let instance_id =
            env::var("AGGREGATOR_INSTANCE_ID").unwrap_or_else(|_| default_instance_id());

        Self {
            lease_ttl,
            renew_margin,
            retry_backoff,
            instance_id,
        }
    }

    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.retry_backoff = backoff;
        self
    }

    pub fn apply_directive(&mut self, directive: &str) -> Result<(), LeaderElectionError> {
        let Some((key, value)) = directive.split_once('=') else {
            return Ok(());
        };
        match key.trim() {
            "instance" => {
                self.instance_id = value.trim().to_owned();
            }
            "ttl_ms" => {
                let ms = value.trim().parse::<u64>().map_err(|err| {
                    LeaderElectionError::InvalidConfig(format!(
                        "invalid ttl_ms value '{value}': {err}"
                    ))
                })?;
                self.lease_ttl = Duration::from_millis(ms);
            }
            "ttl_secs" => {
                let secs = value.trim().parse::<u64>().map_err(|err| {
                    LeaderElectionError::InvalidConfig(format!(
                        "invalid ttl_secs value '{value}': {err}"
                    ))
                })?;
                self.lease_ttl = Duration::from_secs(secs);
            }
            "renew_margin_ms" => {
                let ms = value.trim().parse::<u64>().map_err(|err| {
                    LeaderElectionError::InvalidConfig(format!(
                        "invalid renew_margin_ms value '{value}': {err}"
                    ))
                })?;
                self.renew_margin = Duration::from_millis(ms);
            }
            "renew_margin_secs" => {
                let secs = value.trim().parse::<u64>().map_err(|err| {
                    LeaderElectionError::InvalidConfig(format!(
                        "invalid renew_margin_secs value '{value}': {err}"
                    ))
                })?;
                self.renew_margin = Duration::from_secs(secs);
            }
            "retry_ms" => {
                let ms = value.trim().parse::<u64>().map_err(|err| {
                    LeaderElectionError::InvalidConfig(format!(
                        "invalid retry_ms value '{value}': {err}"
                    ))
                })?;
                self.retry_backoff = Duration::from_millis(ms);
            }
            _ => {
                warn!(
                    target = "aggregator",
                    directive = directive.trim(),
                    "ignored unknown leader-election override"
                );
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), LeaderElectionError> {
        if self.lease_ttl < MIN_SLEEP {
            return Err(LeaderElectionError::InvalidConfig(
                "lease TTL must be at least 250ms".into(),
            ));
        }
        if self.renew_margin >= self.lease_ttl {
            return Err(LeaderElectionError::InvalidConfig(
                "renew margin must be smaller than lease TTL".into(),
            ));
        }
        Ok(())
    }
}

impl Default for LeaderElectionConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug)]
pub enum LeaderElectionError {
    Storage(storage_engine::StorageError),
    Encoding(SerializationError),
    Time(std::time::SystemTimeError),
    InvalidConfig(String),
}

impl fmt::Display for LeaderElectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeaderElectionError::Storage(err) => write!(f, "storage error: {err}"),
            LeaderElectionError::Encoding(err) => write!(f, "encoding error: {err}"),
            LeaderElectionError::Time(err) => write!(f, "time error: {err}"),
            LeaderElectionError::InvalidConfig(err) => {
                write!(f, "invalid leader election configuration: {err}")
            }
        }
    }
}

impl StdError for LeaderElectionError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            LeaderElectionError::Storage(err) => Some(err),
            LeaderElectionError::Encoding(err) => Some(err),
            LeaderElectionError::Time(err) => Some(err),
            LeaderElectionError::InvalidConfig(_) => None,
        }
    }
}

impl From<storage_engine::StorageError> for LeaderElectionError {
    fn from(value: storage_engine::StorageError) -> Self {
        LeaderElectionError::Storage(value)
    }
}

impl From<SerializationError> for LeaderElectionError {
    fn from(value: SerializationError) -> Self {
        LeaderElectionError::Encoding(value)
    }
}

impl From<std::time::SystemTimeError> for LeaderElectionError {
    fn from(value: std::time::SystemTimeError) -> Self {
        LeaderElectionError::Time(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct LeaderRecord {
    holder: String,
    expires_at_ms: u64,
    fencing: u64,
}

pub async fn run_with_options(options: Vec<String>, state: AppState) {
    let mut config = LeaderElectionConfig::from_env();
    for directive in options {
        if let Err(err) = config.apply_directive(&directive) {
            warn!(target = "aggregator", %err, "failed to apply leader-election override");
        }
    }
    run_with_config(state, config).await;
}

pub async fn run_with_config(state: AppState, config: LeaderElectionConfig) {
    match LeaderElection::new(state, config) {
        Ok(election) => election.run().await,
        Err(err) => warn!(target = "aggregator", %err, "failed to initialize leader election"),
    }
}

struct LeaderElection {
    state: AppState,
    store: Arc<InhouseEngine>,
    db_path: Arc<PathBuf>,
    config: LeaderElectionConfig,
}

struct StepOutcome {
    next_poll: Duration,
}

impl LeaderElection {
    fn new(state: AppState, config: LeaderElectionConfig) -> Result<Self, LeaderElectionError> {
        config.validate()?;
        let store = state.store_handle();
        store.ensure_cf(LEADER_CF)?;
        let db_path = state.db_path();
        Ok(Self {
            state,
            store,
            db_path,
            config,
        })
    }

    fn leader_poll_interval(&self) -> Duration {
        self.config
            .lease_ttl
            .checked_sub(self.config.renew_margin)
            .unwrap_or(MIN_SLEEP)
            .max(MIN_SLEEP)
    }

    fn follower_poll_interval(&self) -> Duration {
        if self.config.retry_backoff < MIN_SLEEP {
            MIN_SLEEP
        } else {
            self.config.retry_backoff
        }
    }

    fn step(&self, now: SystemTime) -> Result<StepOutcome, LeaderElectionError> {
        let now_ms = to_millis(now)?;
        let ttl_ms = duration_to_millis(self.config.lease_ttl);
        let renew_margin_ms = duration_to_millis(self.config.renew_margin);
        let mut current = self.read_record()?;

        if let Some(ref mut record) = current {
            if record.holder == self.config.instance_id {
                if record.expires_at_ms.saturating_sub(now_ms) <= renew_margin_ms {
                    record.expires_at_ms = now_ms.saturating_add(ttl_ms);
                    self.write_record(record)?;
                }
                self.state
                    .update_leader_state(true, Some(record.holder.clone()), record.fencing);
                return Ok(StepOutcome {
                    next_poll: self.leader_poll_interval(),
                });
            }

            if record.expires_at_ms <= now_ms {
                let new_record = LeaderRecord {
                    holder: self.config.instance_id.clone(),
                    expires_at_ms: now_ms.saturating_add(ttl_ms),
                    fencing: record.fencing.saturating_add(1),
                };
                self.write_record(&new_record)?;
                current = self.read_record()?;
                if let Some(ref confirmed) = current {
                    let is_self = confirmed.holder == self.config.instance_id;
                    self.state.update_leader_state(
                        is_self,
                        Some(confirmed.holder.clone()),
                        confirmed.fencing,
                    );
                    return Ok(StepOutcome {
                        next_poll: if is_self {
                            self.leader_poll_interval()
                        } else {
                            self.follower_poll_interval()
                        },
                    });
                } else {
                    self.state.update_leader_state(false, None, 0);
                    return Ok(StepOutcome {
                        next_poll: self.follower_poll_interval(),
                    });
                }
            }

            self.state
                .update_leader_state(false, Some(record.holder.clone()), record.fencing);
            return Ok(StepOutcome {
                next_poll: self.follower_poll_interval(),
            });
        }

        let new_record = LeaderRecord {
            holder: self.config.instance_id.clone(),
            expires_at_ms: now_ms.saturating_add(ttl_ms),
            fencing: 1,
        };
        self.write_record(&new_record)?;
        current = self.read_record()?;
        if let Some(ref confirmed) = current {
            let is_self = confirmed.holder == self.config.instance_id;
            self.state.update_leader_state(
                is_self,
                Some(confirmed.holder.clone()),
                confirmed.fencing,
            );
            return Ok(StepOutcome {
                next_poll: if is_self {
                    self.leader_poll_interval()
                } else {
                    self.follower_poll_interval()
                },
            });
        }

        self.state.update_leader_state(false, None, 0);
        Ok(StepOutcome {
            next_poll: self.follower_poll_interval(),
        })
    }

    fn read_record(&self) -> Result<Option<LeaderRecord>, LeaderElectionError> {
        let db_str = self.db_path.as_ref().to_string_lossy();
        let reader = InhouseEngine::open(db_str.as_ref())?;
        reader.ensure_cf(LEADER_CF)?;
        let raw = reader.get(LEADER_CF, LEADER_KEY)?;
        self.decode_record(raw)
    }

    fn decode_record(
        &self,
        raw: Option<Vec<u8>>,
    ) -> Result<Option<LeaderRecord>, LeaderElectionError> {
        if let Some(bytes) = raw {
            match json::from_slice::<LeaderRecord>(&bytes) {
                Ok(record) => Ok(Some(record)),
                Err(err) => {
                    warn!(
                        target = "aggregator",
                        %err,
                        "failed to decode leader lease; clearing entry"
                    );
                    self.store.delete(LEADER_CF, LEADER_KEY)?;
                    self.store.flush()?;
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    fn write_record(&self, record: &LeaderRecord) -> Result<(), LeaderElectionError> {
        let bytes = json::to_vec(record)?;
        self.store.put_bytes(LEADER_CF, LEADER_KEY, &bytes)?;
        self.store.flush()?;
        Ok(())
    }

    async fn run(self) {
        let mut last_snapshot = self.state.leader_snapshot();
        loop {
            let next_poll = match self.step(SystemTime::now()) {
                Ok(outcome) => outcome.next_poll,
                Err(err) => {
                    warn!(target = "aggregator", %err, "leader election step failed");
                    self.state.update_leader_state(false, None, 0);
                    self.follower_poll_interval()
                }
            };

            let snapshot = self.state.leader_snapshot();
            log_transition(&self.config.instance_id, &last_snapshot, &snapshot);
            last_snapshot = snapshot;
            sleep(next_poll).await;
        }
    }
}

fn log_transition(instance_id: &str, previous: &LeaderSnapshot, current: &LeaderSnapshot) {
    if current.is_leader && (!previous.is_leader || previous.leader_id != current.leader_id) {
        info!(
            target = "aggregator",
            fencing = current.fencing_token,
            leader = current.leader_id.as_deref().unwrap_or(instance_id),
            "metrics aggregator assumed leadership"
        );
    } else if !current.is_leader && previous.is_leader {
        info!(
            target = "aggregator",
            "metrics aggregator relinquished leadership"
        );
    } else if !current.is_leader
        && current.leader_id != previous.leader_id
        && current.leader_id.is_some()
    {
        info!(
            target = "aggregator",
            leader = current.leader_id.as_deref().unwrap_or("(none)"),
            fencing = current.fencing_token,
            "observed new remote metrics aggregator leader"
        );
    }
}

fn duration_to_millis(duration: Duration) -> u64 {
    let millis = duration.as_millis();
    if millis == 0 {
        1
    } else if millis > u64::MAX as u128 {
        u64::MAX
    } else {
        millis as u64
    }
}

fn to_millis(now: SystemTime) -> Result<u64, LeaderElectionError> {
    let duration = now.duration_since(UNIX_EPOCH)?;
    Ok(duration_to_millis(duration))
}

fn default_instance_id() -> String {
    let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "aggregator".into());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{hostname}-{pid}-{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile;

    fn state(db: &std::path::Path) -> AppState {
        AppState::new("token".into(), db, 60)
    }

    fn config(instance: &str) -> LeaderElectionConfig {
        LeaderElectionConfig::new(
            instance.to_owned(),
            Duration::from_secs(4),
            Duration::from_secs(1),
        )
        .with_retry_backoff(Duration::from_millis(200))
    }

    #[test]
    fn first_instance_claims_leadership() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("db");
        let state_a = state(&db_path);
        let election = LeaderElection::new(state_a.clone(), config("a")).unwrap();
        let _ = election.step(SystemTime::now()).unwrap();
        let snapshot = state_a.leader_snapshot();
        assert!(snapshot.is_leader);
        assert_eq!(snapshot.leader_id.as_deref(), Some("a"));
    }

    #[test]
    fn follower_observes_existing_leader() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("db");
        let state_a = state(&db_path);
        let election_a = LeaderElection::new(state_a.clone(), config("a")).unwrap();
        let _ = election_a.step(SystemTime::now()).unwrap();

        let state_b = state(&db_path);
        let election_b = LeaderElection::new(state_b.clone(), config("b")).unwrap();
        let _ = election_b.step(SystemTime::now()).unwrap();

        let snapshot_a = state_a.leader_snapshot();
        let snapshot_b = state_b.leader_snapshot();
        assert!(snapshot_a.is_leader);
        assert!(!snapshot_b.is_leader);
        assert_eq!(snapshot_b.leader_id.as_deref(), Some("a"));
    }

    #[test]
    fn follower_takes_over_after_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("db");
        let state_a = state(&db_path);
        let election_a = LeaderElection::new(state_a.clone(), config("a")).unwrap();
        let _ = election_a.step(SystemTime::now()).unwrap();

        let state_b = state(&db_path);
        let election_b = LeaderElection::new(state_b.clone(), config("b")).unwrap();
        let takeover_time = SystemTime::now()
            .checked_add(Duration::from_secs(6))
            .unwrap();
        let _ = election_b.step(takeover_time).unwrap();
        let snapshot_b = state_b.leader_snapshot();
        assert!(snapshot_b.is_leader);
        assert_eq!(snapshot_b.leader_id.as_deref(), Some("b"));

        let _ = election_a.step(takeover_time).unwrap();
        let snapshot_a = state_a.leader_snapshot();
        assert!(!snapshot_a.is_leader);
        assert_eq!(snapshot_a.leader_id.as_deref(), Some("b"));
    }
}

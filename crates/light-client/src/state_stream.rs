use foundation_serialization::{binary, Deserialize, Serialize};
use state::Proof;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, warn};

use crate::config::{load_user_config, state_cache_path, LightClientConfig};

const DEFAULT_LAG_THRESHOLD: u64 = 8;
const DEFAULT_MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

#[cfg(feature = "telemetry")]
mod telemetry {
    use once_cell::sync::Lazy;
    use prometheus::{IntCounter, Opts, Registry};

    pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

    pub static LIGHT_STATE_SNAPSHOT_COMPRESSED_BYTES: Lazy<IntCounter> = Lazy::new(|| {
        let counter = IntCounter::with_opts(
            Opts::new(
                "light_state_snapshot_compressed_bytes",
                "Total compressed bytes processed for light-client snapshots",
            )
            .namespace("the_block"),
        )
        .expect("compressed snapshot counter");
        REGISTRY
            .register(Box::new(counter.clone()))
            .expect("register compressed snapshot counter");
        counter
    });

    pub static LIGHT_STATE_SNAPSHOT_DECOMPRESSED_BYTES: Lazy<IntCounter> = Lazy::new(|| {
        let counter = IntCounter::with_opts(
            Opts::new(
                "light_state_snapshot_decompressed_bytes",
                "Total decompressed bytes applied from light-client snapshots",
            )
            .namespace("the_block"),
        )
        .expect("decompressed snapshot counter");
        REGISTRY
            .register(Box::new(counter.clone()))
            .expect("register decompressed snapshot counter");
        counter
    });

    pub static LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
        let counter = IntCounter::with_opts(
            Opts::new(
                "light_state_snapshot_decompress_errors_total",
                "Total lz77-rle decompression failures for light-client snapshots",
            )
            .namespace("the_block"),
        )
        .expect("snapshot decompression counter");
        REGISTRY
            .register(Box::new(counter.clone()))
            .expect("register snapshot decompression counter");
        counter
    });
}

#[cfg(feature = "telemetry")]
pub use telemetry::{
    LIGHT_STATE_SNAPSHOT_COMPRESSED_BYTES, LIGHT_STATE_SNAPSHOT_DECOMPRESSED_BYTES,
    LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL, REGISTRY as STATE_STREAM_TELEMETRY_REGISTRY,
};

/// Account-level update carried by a [`StateChunk`].
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AccountChunk {
    pub address: String,
    pub balance: u64,
    pub account_seq: u64,
    #[serde(with = "proof_serde")]
    pub proof: Proof,
}

/// A chunk of state updates delivered over the stream.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct StateChunk {
    /// Monotonic sequence number.
    pub seq: u64,
    /// Optional latest chain height for lag detection.
    pub tip_height: u64,
    /// Updated account balances keyed by address along with proofs.
    pub accounts: Vec<AccountChunk>,
    /// Merkle root of the accounts in this chunk.
    pub root: [u8; 32],
    /// Indicates if this chunk is a full snapshot compressed with `lz77-rle`.
    pub compressed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
struct CachedAccount {
    balance: u64,
    seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
struct PersistedState {
    accounts: HashMap<String, CachedAccount>,
    next_seq: u64,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct PersistedStateRef<'a> {
    accounts: &'a HashMap<String, CachedAccount>,
    next_seq: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
struct SnapshotAccount {
    address: String,
    balance: u64,
    seq: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
struct SnapshotPayload {
    accounts: Vec<SnapshotAccount>,
    next_seq: u64,
}

/// Callback invoked when the client detects a sequence gap.
pub type GapCallback = Box<dyn FnMut(u64, u64) -> Result<Vec<StateChunk>, StateStreamError> + Send>;

/// Client-side helper maintaining a rolling cache of account state.
pub struct StateStream {
    cache: HashMap<String, CachedAccount>,
    next_seq: u64,
    lag_threshold: u64,
    gap_fetcher: Option<GapCallback>,
    persist_path: Option<PathBuf>,
    max_snapshot_bytes: usize,
}

/// Builder for [`StateStream`].
pub struct StateStreamBuilder {
    cache_path: Option<PathBuf>,
    lag_threshold: u64,
    max_snapshot_bytes: usize,
    gap_fetcher: Option<GapCallback>,
}

#[derive(Debug, Error)]
pub enum StateStreamError {
    #[error("sequence mismatch: expected {expected}, received {received}")]
    SequenceMismatch { expected: u64, received: u64 },
    #[error("invalid proof for address {address}")]
    InvalidProof { address: String },
    #[error("stale update for {address}: cached sequence {cached_seq}, received {update_seq}")]
    StaleAccountUpdate {
        address: String,
        cached_seq: u64,
        update_seq: u64,
    },
    #[error("gap detected: expected {expected}, received {received}")]
    GapDetected { expected: u64, received: u64 },
    #[error("gap recovery incomplete: expected {expected}, recovered {recovered}")]
    GapRecoveryFailed { expected: u64, recovered: u64 },
    #[error("snapshot exceeds max size (size={size}, limit={limit})")]
    SnapshotTooLarge { size: usize, limit: usize },
    #[error("failed to decode snapshot payload: {0}")]
    SnapshotDecode(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StateStreamBuilder {
    pub fn new() -> Self {
        Self {
            cache_path: None,
            lag_threshold: DEFAULT_LAG_THRESHOLD,
            max_snapshot_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
            gap_fetcher: None,
        }
    }

    pub fn cache_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.cache_path = Some(path.into());
        self
    }

    pub fn lag_threshold(mut self, threshold: u64) -> Self {
        self.lag_threshold = threshold;
        self
    }

    pub fn max_snapshot_bytes(mut self, bytes: usize) -> Self {
        self.max_snapshot_bytes = bytes;
        self
    }

    pub fn gap_fetcher<F>(mut self, fetcher: F) -> Self
    where
        F: FnMut(u64, u64) -> Result<Vec<StateChunk>, StateStreamError> + Send + 'static,
    {
        self.gap_fetcher = Some(Box::new(fetcher));
        self
    }

    pub fn build(self) -> StateStream {
        StateStream::from_builder(self)
    }
}

impl Default for StateStreamBuilder {
    fn default() -> Self {
        StateStreamBuilder::new()
    }
}

impl StateStream {
    /// Create a new stream using persisted state (if any) and configuration defaults.
    pub fn new() -> Self {
        let config = load_user_config().unwrap_or_else(|err| {
            warn!("failed to load light client config: {err}");
            LightClientConfig::default()
        });
        Self::from_config(&config)
    }

    /// Create a new stream deriving limits from the provided configuration.
    pub fn from_config(config: &LightClientConfig) -> Self {
        let mut builder =
            StateStreamBuilder::new().max_snapshot_bytes(config.snapshot_limit_bytes());
        if let Some(path) = state_cache_path() {
            builder = builder.cache_path(path);
        }
        builder.build()
    }

    pub fn builder() -> StateStreamBuilder {
        StateStreamBuilder::new()
    }

    fn from_builder(builder: StateStreamBuilder) -> Self {
        let mut cache = HashMap::new();
        let mut next_seq = 0u64;
        if let Some(path) = builder.cache_path.as_ref() {
            match Self::load_persisted(path) {
                Ok(Some(state)) => {
                    cache = state.accounts;
                    next_seq = state.next_seq;
                }
                Ok(None) => {}
                Err(err) => warn!("failed to load persisted light-client cache: {err}"),
            }
        }
        Self {
            cache,
            next_seq,
            lag_threshold: if builder.lag_threshold == 0 {
                DEFAULT_LAG_THRESHOLD
            } else {
                builder.lag_threshold
            },
            gap_fetcher: builder.gap_fetcher,
            persist_path: builder.cache_path,
            max_snapshot_bytes: if builder.max_snapshot_bytes == 0 {
                DEFAULT_MAX_SNAPSHOT_BYTES
            } else {
                builder.max_snapshot_bytes
            },
        }
    }

    fn load_persisted(path: &Path) -> Result<Option<PersistedState>, StateStreamError> {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        match binary::decode::<PersistedState>(&bytes) {
            Ok(state) => Ok(Some(state)),
            Err(_) => {
                if let Ok(legacy) = binary::decode::<HashMap<String, u64>>(&bytes) {
                    let accounts = legacy
                        .into_iter()
                        .map(|(addr, bal)| {
                            (
                                addr,
                                CachedAccount {
                                    balance: bal,
                                    seq: 0,
                                },
                            )
                        })
                        .collect();
                    Ok(Some(PersistedState {
                        accounts,
                        next_seq: 0,
                    }))
                } else {
                    Err(StateStreamError::Serialization(
                        "unable to decode persisted cache".to_string(),
                    ))
                }
            }
        }
    }

    fn persist_state(&self) -> Result<(), StateStreamError> {
        if let Some(path) = &self.persist_path {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let tmp_path = path.with_extension("tmp");
            let state = PersistedStateRef {
                accounts: &self.cache,
                next_seq: self.next_seq,
            };
            let bytes = binary::encode(&state)
                .map_err(|err| StateStreamError::Serialization(err.to_string()))?;
            fs::write(&tmp_path, &bytes)?;
            if let Err(err) = fs::rename(&tmp_path, path) {
                let _ = fs::remove_file(&tmp_path);
                return Err(StateStreamError::Io(err));
            }
        }
        Ok(())
    }

    pub fn set_gap_fetcher<F>(&mut self, fetcher: F)
    where
        F: FnMut(u64, u64) -> Result<Vec<StateChunk>, StateStreamError> + Send + 'static,
    {
        self.gap_fetcher = Some(Box::new(fetcher));
    }

    pub fn lag_threshold(&self) -> u64 {
        self.lag_threshold
    }

    pub fn set_lag_threshold(&mut self, threshold: u64) {
        self.lag_threshold = if threshold == 0 {
            DEFAULT_LAG_THRESHOLD
        } else {
            threshold
        };
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Apply an incremental chunk of updates. Returns an error when validation fails.
    pub fn apply_chunk(&mut self, chunk: StateChunk) -> Result<(), StateStreamError> {
        if chunk.seq > self.next_seq {
            self.recover_gap(chunk.seq)?;
        }
        if chunk.seq < self.next_seq {
            return Err(StateStreamError::SequenceMismatch {
                expected: self.next_seq,
                received: chunk.seq,
            });
        }
        self.apply_chunk_inner(chunk)
    }

    fn recover_gap(&mut self, target_seq: u64) -> Result<(), StateStreamError> {
        if self.gap_fetcher.is_none() {
            return Err(StateStreamError::GapDetected {
                expected: self.next_seq,
                received: target_seq,
            });
        }
        while self.next_seq < target_seq {
            let missing = {
                // Drop the mutable borrow of the callback before applying the chunks so
                // we can borrow `self` mutably again.
                let fetcher = self
                    .gap_fetcher
                    .as_mut()
                    .expect("gap fetcher present while recovering");
                fetcher(self.next_seq, target_seq)?
            };
            if missing.is_empty() {
                break;
            }
            for chunk in missing {
                if chunk.seq != self.next_seq {
                    return Err(StateStreamError::SequenceMismatch {
                        expected: self.next_seq,
                        received: chunk.seq,
                    });
                }
                self.apply_chunk_inner(chunk)?;
            }
        }
        if self.next_seq < target_seq {
            return Err(StateStreamError::GapRecoveryFailed {
                expected: target_seq,
                recovered: self.next_seq,
            });
        }
        Ok(())
    }

    fn apply_chunk_inner(&mut self, chunk: StateChunk) -> Result<(), StateStreamError> {
        if chunk.seq != self.next_seq {
            return Err(StateStreamError::SequenceMismatch {
                expected: self.next_seq,
                received: chunk.seq,
            });
        }
        let root = chunk.root;
        let accounts = chunk.accounts;
        let mut previous: HashMap<String, Option<CachedAccount>> = HashMap::new();
        for account in &accounts {
            previous
                .entry(account.address.clone())
                .or_insert_with(|| self.cache.get(&account.address).cloned());
            let value = account_state_value(account.balance, account.account_seq);
            if !state::MerkleTrie::verify_proof(
                root,
                account.address.as_bytes(),
                &value,
                &account.proof,
            ) {
                return Err(StateStreamError::InvalidProof {
                    address: account.address.clone(),
                });
            }
            if let Some(existing) = self.cache.get(&account.address) {
                if account.account_seq < existing.seq {
                    return Err(StateStreamError::StaleAccountUpdate {
                        address: account.address.clone(),
                        cached_seq: existing.seq,
                        update_seq: account.account_seq,
                    });
                }
            }
        }
        for account in accounts {
            self.cache.insert(
                account.address,
                CachedAccount {
                    balance: account.balance,
                    seq: account.account_seq,
                },
            );
        }
        let prev_next_seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        if let Err(err) = self.persist_state() {
            self.next_seq = prev_next_seq;
            for (address, prior) in previous {
                match prior {
                    Some(entry) => {
                        self.cache.insert(address, entry);
                    }
                    None => {
                        self.cache.remove(&address);
                    }
                }
            }
            return Err(err);
        }
        Ok(())
    }

    /// Apply a full snapshot, optionally compressed with `lz77-rle`.
    pub fn apply_snapshot(
        &mut self,
        data: &[u8],
        compressed: bool,
    ) -> Result<(), StateStreamError> {
        if data.len() > self.max_snapshot_bytes {
            return Err(StateStreamError::SnapshotTooLarge {
                size: data.len(),
                limit: self.max_snapshot_bytes,
            });
        }
        let bytes = if compressed {
            #[cfg(feature = "telemetry")]
            LIGHT_STATE_SNAPSHOT_COMPRESSED_BYTES.inc_by(data.len() as u64);
            match coding::compressor_for("lz77-rle", 4) {
                Ok(compressor) => match compressor.decompress(data) {
                    Ok(decoded) => decoded,
                    Err(err) => {
                        #[cfg(feature = "telemetry")]
                        LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL.inc();
                        return Err(StateStreamError::SnapshotDecode(err.to_string()));
                    }
                },
                Err(err) => {
                    #[cfg(feature = "telemetry")]
                    LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL.inc();
                    return Err(StateStreamError::SnapshotDecode(err.to_string()));
                }
            }
        } else {
            data.to_vec()
        };
        if bytes.len() > self.max_snapshot_bytes {
            return Err(StateStreamError::SnapshotTooLarge {
                size: bytes.len(),
                limit: self.max_snapshot_bytes,
            });
        }
        #[cfg(feature = "telemetry")]
        LIGHT_STATE_SNAPSHOT_DECOMPRESSED_BYTES.inc_by(bytes.len() as u64);
        let payload = Self::decode_snapshot(&bytes)?;
        let mut cache = HashMap::with_capacity(payload.accounts.len());
        for account in payload.accounts {
            cache.insert(
                account.address,
                CachedAccount {
                    balance: account.balance,
                    seq: account.seq,
                },
            );
        }
        let previous_cache = std::mem::replace(&mut self.cache, cache);
        let previous_seq = std::mem::replace(&mut self.next_seq, payload.next_seq);
        if let Err(err) = self.persist_state() {
            self.cache = previous_cache;
            self.next_seq = previous_seq;
            return Err(err);
        }
        debug!(
            snapshot_accounts = self.cache.len(),
            next_seq = self.next_seq,
            "applied state snapshot"
        );
        Ok(())
    }

    fn decode_snapshot(bytes: &[u8]) -> Result<SnapshotPayload, StateStreamError> {
        match binary::decode::<SnapshotPayload>(bytes) {
            Ok(snapshot) => Ok(snapshot),
            Err(_) => {
                if let Ok(legacy) = binary::decode::<HashMap<String, u64>>(bytes) {
                    let accounts = legacy
                        .into_iter()
                        .map(|(address, balance)| SnapshotAccount {
                            address,
                            balance,
                            seq: 0,
                        })
                        .collect();
                    Ok(SnapshotPayload {
                        accounts,
                        next_seq: 0,
                    })
                } else {
                    Err(StateStreamError::SnapshotDecode(
                        "unable to decode snapshot payload".to_string(),
                    ))
                }
            }
        }
    }

    /// Returns true if the client is behind the provided chain height by more
    /// than the configured lag threshold.
    pub fn lagging(&self, tip_height: u64) -> bool {
        let lag = tip_height.saturating_sub(self.next_seq);
        let lagging = lag > self.lag_threshold;
        if lagging {
            warn!(
                lag_blocks = lag,
                lag_threshold = self.lag_threshold,
                next_seq = self.next_seq,
                tip_height,
                "light-client stream is lagging"
            );
        }
        lagging
    }

    pub fn cached_balance(&self, address: &str) -> Option<(u64, u64)> {
        self.cache
            .get(address)
            .map(|entry| (entry.balance, entry.seq))
    }
}

/// Encode an account balance/sequence pair for Merkle verification.
pub fn account_state_value(balance: u64, seq: u64) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&balance.to_le_bytes());
    bytes[8..].copy_from_slice(&seq.to_le_bytes());
    bytes
}

mod proof_serde {
    use super::*;
    use foundation_serialization::serde::{
        de::{SeqAccess, Visitor},
        ser::SerializeSeq,
        Deserializer, Serializer,
    };
    use std::fmt;

    pub fn serialize<S>(proof: &Proof, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(proof.0.len()))?;
        for (hash, is_left) in &proof.0 {
            seq.serialize_element(&(*hash, *is_left))?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Proof, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ProofVisitor;

        impl<'de> Visitor<'de> for ProofVisitor {
            type Value = Proof;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a Merkle proof as a sequence of (hash, is_left)")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((hash, is_left)) = seq.next_element::<([u8; 32], bool)>()? {
                    entries.push((hash, is_left));
                }
                Ok(Proof(entries))
            }
        }

        deserializer.deserialize_seq(ProofVisitor)
    }
}

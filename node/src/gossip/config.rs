use concurrency::Lazy;
use foundation_serialization::toml;
use foundation_serialization::Deserialize;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Runtime tunables for the gossip relay deduplication and fanout heuristics.
#[derive(Clone, Deserialize, Debug)]
pub struct GossipConfig {
    #[serde(default = "GossipConfig::default_ttl_ms")]
    pub ttl_ms: u64,
    #[serde(default = "GossipConfig::default_dedup_capacity")]
    pub dedup_capacity: usize,
    #[serde(default = "GossipConfig::default_min_fanout")]
    pub min_fanout: usize,
    #[serde(default = "GossipConfig::default_base_fanout")]
    pub base_fanout: usize,
    #[serde(default = "GossipConfig::default_max_fanout")]
    pub max_fanout: usize,
    #[serde(default = "GossipConfig::default_failure_penalty")]
    pub failure_penalty: f64,
    #[serde(default = "GossipConfig::default_latency_weight")]
    pub latency_weight: f64,
    #[serde(default = "GossipConfig::default_reputation_weight")]
    pub reputation_weight: f64,
    #[serde(default = "GossipConfig::default_latency_baseline_ms")]
    pub latency_baseline_ms: u64,
    #[serde(default = "GossipConfig::default_low_score_cutoff")]
    pub low_score_cutoff: f64,
    #[serde(default = "GossipConfig::default_shard_store_path")]
    pub shard_store_path: String,
}

impl GossipConfig {
    fn default_ttl_ms() -> u64 {
        2_000
    }

    fn default_dedup_capacity() -> usize {
        8192
    }

    fn default_min_fanout() -> usize {
        3
    }

    fn default_base_fanout() -> usize {
        8
    }

    fn default_max_fanout() -> usize {
        24
    }

    fn default_failure_penalty() -> f64 {
        1.5
    }

    fn default_latency_weight() -> f64 {
        0.6
    }

    fn default_reputation_weight() -> f64 {
        1.0
    }

    fn default_latency_baseline_ms() -> u64 {
        40
    }

    fn default_low_score_cutoff() -> f64 {
        0.55
    }

    fn default_shard_store_path() -> String {
        "state/gossip_shards".to_string()
    }

    fn load_from(path: &Path) -> Option<Self> {
        fs::read_to_string(path)
            .ok()
            .and_then(|data| toml::from_str(&data).ok())
    }

    fn validate(mut self) -> Self {
        if self.min_fanout == 0 {
            self.min_fanout = 1;
        }
        if self.base_fanout < self.min_fanout {
            self.base_fanout = self.min_fanout;
        }
        if self.max_fanout < self.base_fanout {
            self.max_fanout = self.base_fanout.max(self.min_fanout);
        }
        if self.dedup_capacity == 0 {
            self.dedup_capacity = Self::default_dedup_capacity();
        }
        if self.low_score_cutoff <= 0.0 {
            self.low_score_cutoff = Self::default_low_score_cutoff();
        }
        if self.failure_penalty < 0.0 {
            self.failure_penalty = Self::default_failure_penalty();
        }
        if self.latency_weight < 0.0 {
            self.latency_weight = Self::default_latency_weight();
        }
        if self.reputation_weight <= 0.0 {
            self.reputation_weight = Self::default_reputation_weight();
        }
        if self.latency_baseline_ms == 0 {
            self.latency_baseline_ms = Self::default_latency_baseline_ms();
        }
        self
    }

    /// Return the deduplication capacity as a [`NonZeroUsize`] for cache sizing.
    pub fn dedup_capacity(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.dedup_capacity).unwrap_or_else(|| {
            NonZeroUsize::new(Self::default_dedup_capacity()).expect("non-zero capacity")
        })
    }
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            ttl_ms: Self::default_ttl_ms(),
            dedup_capacity: Self::default_dedup_capacity(),
            min_fanout: Self::default_min_fanout(),
            base_fanout: Self::default_base_fanout(),
            max_fanout: Self::default_max_fanout(),
            failure_penalty: Self::default_failure_penalty(),
            latency_weight: Self::default_latency_weight(),
            reputation_weight: Self::default_reputation_weight(),
            latency_baseline_ms: Self::default_latency_baseline_ms(),
            low_score_cutoff: Self::default_low_score_cutoff(),
            shard_store_path: Self::default_shard_store_path(),
        }
    }
}

static CURRENT: Lazy<RwLock<GossipConfig>> = Lazy::new(|| RwLock::new(GossipConfig::default()));
static DIR: Lazy<RwLock<Option<String>>> = Lazy::new(|| RwLock::new(None));

fn config_path(dir: &str) -> PathBuf {
    Path::new(dir).join("gossip.toml")
}

fn reload_from_dir(dir: Option<String>) {
    let cfg = dir
        .as_ref()
        .and_then(|d| GossipConfig::load_from(&config_path(d)))
        .unwrap_or_default()
        .validate();
    *CURRENT.write().unwrap() = cfg;
}

/// Persist the active configuration directory for future reloads and prime the
/// current snapshot by reading `gossip.toml` if it exists.
pub fn set_dir(dir: &str) {
    {
        *DIR.write().unwrap() = Some(dir.to_string());
    }
    reload_from_dir(Some(dir.to_string()));
}

/// Snapshot the current gossip relay configuration.
pub fn current() -> GossipConfig {
    CURRENT.read().unwrap().clone()
}

/// Reload configuration from disk using the last configured directory.
pub fn reload() {
    let dir = DIR.read().unwrap().clone();
    reload_from_dir(dir);
}

/// Install filesystem and signal watchers that keep the gossip configuration in
/// sync with `gossip.toml`.
pub fn watch(dir: &str) {
    set_dir(dir);
}

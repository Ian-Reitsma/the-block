use concurrency::Lazy;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Duration;

const BADGE_PHYSICAL_PRESENCE: &str = "physical_presence";

static BADGE_REGISTRY: Lazy<RwLock<HashMap<String, HashSet<String>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn register_badge(provider: &str, badge: &str) {
    let mut guard = BADGE_REGISTRY
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    guard
        .entry(provider.to_string())
        .or_insert_with(HashSet::new)
        .insert(badge.to_string());
}

fn unregister_badge(provider: &str, badge: &str) {
    let mut guard = BADGE_REGISTRY
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(entry) = guard.get_mut(provider) {
        entry.remove(badge);
        if entry.is_empty() {
            guard.remove(provider);
        }
    }
}

pub fn provider_badges(provider: &str) -> Vec<String> {
    let guard = BADGE_REGISTRY
        .read()
        .unwrap_or_else(|poison| poison.into_inner());
    guard
        .get(provider)
        .map(|set| {
            let mut badges: Vec<String> = set.iter().cloned().collect();
            badges.sort();
            badges
        })
        .unwrap_or_default()
}

fn register_physical_presence(provider: &str) {
    register_badge(provider, BADGE_PHYSICAL_PRESENCE);
}

fn revoke_physical_presence(provider: &str) {
    unregister_badge(provider, BADGE_PHYSICAL_PRESENCE);
}

#[cfg(test)]
pub fn set_physical_presence(provider: &str, active: bool) {
    if active {
        register_physical_presence(provider);
    } else {
        revoke_physical_presence(provider);
    }
}

#[cfg(test)]
pub fn clear_badges() {
    BADGE_REGISTRY
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();
}

/// Badge lifetime in seconds; adjustable via governance hooks.
///
/// ```
/// use the_block::service_badge::ServiceBadgeTracker;
/// use std::time::Duration;
/// let mut tracker = ServiceBadgeTracker::default();
/// tracker.record_epoch("node", true, Duration::from_millis(1));
/// assert!(tracker.uptime_percent() > 0.0);
/// ```
static BADGE_TTL_SECS: AtomicU64 = AtomicU64::new(30 * 24 * 60 * 60);
static BADGE_MIN_EPOCHS: AtomicU64 = AtomicU64::new(90);
static BADGE_ISSUE_UPTIME: AtomicU64 = AtomicU64::new(99);
static BADGE_REVOKE_UPTIME: AtomicU64 = AtomicU64::new(95);

/// Tracks uptime and basic performance metrics to determine badge eligibility.
#[derive(Clone, Default)]
pub struct ServiceBadgeTracker {
    uptime_epochs: u64,
    total_epochs: u64,
    badge_minted: bool,
    latency_samples: Vec<Duration>,
    last_mint: Option<u64>,
    last_burn: Option<u64>,
    expiry: Option<u64>,
    token: Option<String>,
    renewals: u64,
    provider: Option<String>,
}

impl ServiceBadgeTracker {
    /// Create a new tracker with no recorded uptime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Assign the provider identifier associated with this tracker.
    pub fn set_provider(&mut self, provider: &str) {
        self.provider = Some(provider.to_string());
    }

    fn register_provider_badge(&self) {
        if let Some(provider) = self.provider.as_deref() {
            register_physical_presence(provider);
        }
    }

    fn revoke_provider_badge(&self) {
        if let Some(provider) = self.provider.as_deref() {
            revoke_physical_presence(provider);
        }
    }

    /// Record a heartbeat proof for the current epoch.
    ///
    /// A valid proof counts toward uptime; missing or invalid proofs are
    /// treated as downtime and may revoke an existing badge.
    pub fn record_epoch(&mut self, provider: &str, proof_ok: bool, latency: Duration) {
        self.provider = Some(provider.to_string());
        self.total_epochs += 1;
        if proof_ok {
            self.uptime_epochs += 1;
        }
        self.latency_samples.push(latency);
        #[cfg(feature = "telemetry")]
        crate::telemetry::COMPUTE_PROVIDER_UPTIME
            .ensure_handle_for_label_values(&[provider])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(self.uptime_percent().round() as i64);
        // Update badge status on each epoch so lapses trigger revocation.
        self.check_badges();
    }

    /// Percentage of epochs where the node was considered up.
    pub fn uptime_percent(&self) -> f64 {
        if self.total_epochs == 0 {
            return 0.0;
        }
        (self.uptime_epochs as f64 / self.total_epochs as f64) * 100.0
    }

    /// Mint or revoke badges based on recorded metrics.
    pub fn check_badges(&mut self) {
        let now = current_ts();
        let min_epochs = BADGE_MIN_EPOCHS.load(Ordering::Relaxed);
        let issue_pct = BADGE_ISSUE_UPTIME.load(Ordering::Relaxed) as f64;
        let revoke_pct = BADGE_REVOKE_UPTIME.load(Ordering::Relaxed) as f64;

        if !self.badge_minted {
            if self.total_epochs >= min_epochs && self.uptime_percent() >= issue_pct {
                self.issue_badge();
            }
        } else {
            let expired = self.expiry.map_or(false, |e| now >= e);
            if expired || self.uptime_percent() < revoke_pct {
                self.revoke_badge();
            }
        }
    }

    /// Force badge issuance and return the token.
    fn issue_badge(&mut self) -> String {
        let now = current_ts();
        let ttl = BADGE_TTL_SECS.load(Ordering::Relaxed);
        let exp = now + ttl;
        let token = format!("{:x}", exp);
        self.badge_minted = true;
        self.last_mint = Some(now);
        self.expiry = Some(exp);
        self.token = Some(token.clone());
        self.register_provider_badge();
        #[cfg(feature = "telemetry")]
        crate::telemetry::BADGE_ISSUED_TOTAL.inc();
        token
    }

    /// Renew an existing badge, extending its expiry.
    pub fn renew(&mut self) -> Option<String> {
        if self.badge_minted {
            self.renewals = self.renewals.saturating_add(1);
            Some(self.issue_badge())
        } else {
            None
        }
    }

    fn revoke_badge(&mut self) {
        self.badge_minted = false;
        self.token = None;
        self.expiry = None;
        self.last_burn = Some(current_ts());
        self.revoke_provider_badge();
        #[cfg(feature = "telemetry")]
        crate::telemetry::BADGE_REVOKED_TOTAL.inc();
    }

    /// Force revoke a badge.
    pub fn revoke(&mut self) {
        if self.badge_minted {
            self.revoke_badge();
        }
    }

    /// Force badge issuance regardless of uptime.
    pub fn force_issue(&mut self) -> String {
        self.issue_badge()
    }

    /// Whether a badge has been issued.
    pub fn has_badge(&self) -> bool {
        self.badge_minted
    }

    /// Current badge token if issued.
    pub fn current_badge(&self) -> Option<String> {
        self.token.clone()
    }

    pub fn last_mint(&self) -> Option<u64> {
        self.last_mint
    }

    pub fn last_burn(&self) -> Option<u64> {
        self.last_burn
    }

    pub fn renewal_count(&self) -> u64 {
        self.renewals
    }
}

/// Current UNIX timestamp in seconds.
pub fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| panic!("time error: {e}"))
        .as_secs()
}

/// Verify that a badge token is still valid based on its embedded expiry.
pub fn verify(token: &str) -> bool {
    if let Ok(exp) = u64::from_str_radix(token, 16) {
        current_ts() <= exp
    } else {
        false
    }
}

/// Update badge expiry policy (seconds) via governance.
pub fn set_badge_ttl_secs(v: u64) {
    BADGE_TTL_SECS.store(v, Ordering::Relaxed);
}

pub fn set_badge_min_epochs(v: u64) {
    BADGE_MIN_EPOCHS.store(v, Ordering::Relaxed);
}

pub fn set_badge_issue_uptime(v: u64) {
    BADGE_ISSUE_UPTIME.store(v, Ordering::Relaxed);
}

pub fn set_badge_revoke_uptime(v: u64) {
    BADGE_REVOKE_UPTIME.store(v, Ordering::Relaxed);
}

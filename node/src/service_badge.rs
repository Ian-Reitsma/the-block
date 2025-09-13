use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Badge lifetime in seconds; adjustable via governance hooks.
///
/// ```
/// use node::service_badge::ServiceBadgeTracker;
/// use std::time::Duration;
/// let mut tracker = ServiceBadgeTracker::default();
/// tracker.record_epoch("node", true, Duration::from_millis(1));
/// assert!(tracker.uptime_percent() > 0.0);
/// ```
static BADGE_TTL_SECS: AtomicU64 = AtomicU64::new(30 * 24 * 60 * 60);

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
}

impl ServiceBadgeTracker {
    /// Create a new tracker with no recorded uptime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a heartbeat proof for the current epoch.
    ///
    /// A valid proof counts toward uptime; missing or invalid proofs are
    /// treated as downtime and may revoke an existing badge.
    pub fn record_epoch(&mut self, provider: &str, proof_ok: bool, latency: Duration) {
        self.total_epochs += 1;
        if proof_ok {
            self.uptime_epochs += 1;
        }
        self.latency_samples.push(latency);
        #[cfg(feature = "telemetry")]
        crate::telemetry::COMPUTE_PROVIDER_UPTIME
            .with_label_values(&[provider])
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
        if !self.badge_minted {
            if self.total_epochs >= 90 && self.uptime_percent() >= 99.0 {
                self.issue_badge();
            }
        } else {
            let expired = self.expiry.map_or(false, |e| now >= e);
            if expired || self.uptime_percent() < 95.0 {
                self.badge_minted = false;
                self.token = None;
                self.expiry = None;
                self.last_burn = Some(now);
                #[cfg(feature = "telemetry")]
                crate::telemetry::BADGE_REVOKED_TOTAL.inc();
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
        #[cfg(feature = "telemetry")]
        crate::telemetry::BADGE_ISSUED_TOTAL.inc();
        token
    }

    /// Renew an existing badge, extending its expiry.
    pub fn renew(&mut self) -> Option<String> {
        if self.badge_minted {
            Some(self.issue_badge())
        } else {
            None
        }
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

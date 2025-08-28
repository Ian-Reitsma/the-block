use std::time::Duration;

/// Tracks uptime and basic performance metrics to determine badge eligibility.
#[derive(Clone, Default)]
pub struct ServiceBadgeTracker {
    uptime_epochs: u64,
    total_epochs: u64,
    badge_minted: bool,
    latency_samples: Vec<Duration>,
    last_mint: Option<u64>,
    last_burn: Option<u64>,
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
    pub fn record_epoch(&mut self, proof_ok: bool, latency: Duration) {
        self.total_epochs += 1;
        if proof_ok {
            self.uptime_epochs += 1;
        }
        self.latency_samples.push(latency);
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
        if !self.badge_minted {
            if self.total_epochs >= 90 && self.uptime_percent() >= 99.0 {
                self.badge_minted = true;
                self.last_mint = Some(current_ts());
            }
        } else if self.uptime_percent() < 95.0 {
            // Revoke the badge if uptime slips below 95% after minting.
            self.badge_minted = false;
            self.last_burn = Some(current_ts());
        }
    }

    /// Whether a badge has been issued.
    pub fn has_badge(&self) -> bool {
        self.badge_minted
    }

    pub fn last_mint(&self) -> Option<u64> {
        self.last_mint
    }

    pub fn last_burn(&self) -> Option<u64> {
        self.last_burn
    }
}

fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| panic!("time error: {e}"))
        .as_secs()
}

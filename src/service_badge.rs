use std::time::Duration;

/// Tracks uptime and basic performance metrics to determine badge eligibility.
#[derive(Clone, Default)]
pub struct ServiceBadgeTracker {
    uptime_epochs: u64,
    total_epochs: u64,
    badge_minted: bool,
    latency_samples: Vec<Duration>,
}

impl ServiceBadgeTracker {
    /// Create a new tracker with no recorded uptime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record whether the node was up for the given epoch.
    pub fn record_epoch(&mut self, up: bool, latency: Duration) {
        self.total_epochs += 1;
        if up {
            self.uptime_epochs += 1;
        }
        self.latency_samples.push(latency);
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
        if !self.badge_minted && self.total_epochs >= 90 && self.uptime_percent() >= 99.0 {
            self.badge_minted = true;
        }
    }

    /// Whether a badge has been issued.
    pub fn has_badge(&self) -> bool {
        self.badge_minted
    }
}

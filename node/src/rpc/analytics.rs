use serde::{Serialize, Deserialize};

#[derive(Deserialize)]
pub struct AnalyticsQuery {
    pub domain: String,
}

#[derive(Serialize)]
pub struct AnalyticsStats {
    pub reads: u64,
    pub bytes: u64,
}

/// Return aggregated read metrics for a given domain.
pub fn analytics(stats: &crate::telemetry::ReadStats, q: AnalyticsQuery) -> AnalyticsStats {
    let (reads, bytes) = stats.snapshot(&q.domain);
    AnalyticsStats { reads, bytes }
}

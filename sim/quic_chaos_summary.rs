use foundation_serialization::json;
use serde::Serialize;
use std::io::Write;
use std::time::Duration;

/// Single run observation captured by the chaos harness.
#[derive(Debug, Clone, Serialize)]
pub struct ChaosObservation {
    pub loss_rate: f64,
    pub dup_rate: f64,
    pub duration: Duration,
    pub retransmits: u64,
    pub succeeded: bool,
}

/// Aggregated summary statistics for a collection of chaos runs.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct ChaosSummary {
    pub runs: usize,
    pub successes: usize,
    pub avg_duration_ms: f64,
    pub avg_retransmits: f64,
    pub avg_loss_rate: f64,
    pub avg_dup_rate: f64,
}

impl ChaosSummary {
    fn from_observations(records: &[ChaosObservation]) -> Self {
        if records.is_empty() {
            return Self::default();
        }

        let runs = records.len();
        let successes = records.iter().filter(|obs| obs.succeeded).count();
        let total_duration: f64 = records
            .iter()
            .map(|obs| obs.duration.as_secs_f64() * 1000.0)
            .sum();
        let total_retransmits: u64 = records.iter().map(|obs| obs.retransmits).sum();
        let total_loss: f64 = records.iter().map(|obs| obs.loss_rate).sum();
        let total_dup: f64 = records.iter().map(|obs| obs.dup_rate).sum();

        Self {
            runs,
            successes,
            avg_duration_ms: total_duration / runs as f64,
            avg_retransmits: total_retransmits as f64 / runs as f64,
            avg_loss_rate: total_loss / runs as f64,
            avg_dup_rate: total_dup / runs as f64,
        }
    }
}

/// Compute a chaos summary for the provided observations.
pub fn summarize(records: &[ChaosObservation]) -> ChaosSummary {
    ChaosSummary::from_observations(records)
}

/// Serialize a summary to JSON for dashboards or archival.
pub fn write_summary_json(
    records: &[ChaosObservation],
    mut writer: impl Write,
) -> std::io::Result<()> {
    let summary = summarize(records);
    let payload = json::to_vec_pretty(&summary)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    writer.write_all(&payload)?;
    writer.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_is_computed() {
        let observations = vec![
            ChaosObservation {
                loss_rate: 0.05,
                dup_rate: 0.01,
                duration: Duration::from_millis(420),
                retransmits: 3,
                succeeded: true,
            },
            ChaosObservation {
                loss_rate: 0.10,
                dup_rate: 0.02,
                duration: Duration::from_millis(610),
                retransmits: 7,
                succeeded: false,
            },
        ];

        let summary = summarize(&observations);
        assert_eq!(summary.runs, 2);
        assert_eq!(summary.successes, 1);
        assert!((summary.avg_duration_ms - 515.0).abs() < f64::EPSILON);
        assert!((summary.avg_retransmits - 5.0).abs() < f64::EPSILON);
        assert!((summary.avg_loss_rate - 0.075).abs() < f64::EPSILON);
        assert!((summary.avg_dup_rate - 0.015).abs() < f64::EPSILON);
    }

    #[test]
    fn json_writer_serializes() {
        let observations = vec![ChaosObservation {
            loss_rate: 0.02,
            dup_rate: 0.0,
            duration: Duration::from_millis(200),
            retransmits: 1,
            succeeded: true,
        }];

        let mut out = Vec::new();
        write_summary_json(&observations, &mut out).expect("json");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("\"runs\": 1"));
        assert!(text.contains("\"successes\": 1"));
    }
}

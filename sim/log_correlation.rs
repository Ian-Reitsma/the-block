//! Simulate mismatched correlation identifiers between metrics and logs.

pub fn simulate_log_metric_mismatch(pairs: &[(&str, &str)]) {
    let mut matched = 0usize;
    for (metric_id, log_id) in pairs {
        let ok = metric_id == log_id;
        if ok {
            matched += 1;
        }
        println!(
            "metric_correlation={} log_correlation={} match={}",
            metric_id, log_id, ok
        );
    }
    println!("{} of {} correlations matched", matched, pairs.len());
}

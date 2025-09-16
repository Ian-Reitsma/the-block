//! Model slow-release fetch scenarios to highlight rollout skew.

pub fn simulate_lagging_release(latencies: &[u64], timeout: u64) {
    let mut lagging = 0usize;
    for (node, latency) in latencies.iter().enumerate() {
        let behind = latency > &timeout;
        if behind {
            lagging += 1;
        }
        println!(
            "node={} fetch_latency={}s status={}",
            node,
            latency,
            if behind { "lagging" } else { "ok" }
        );
    }
    println!(
        "{} of {} nodes exceeded the {}s fetch SLA",
        lagging,
        latencies.len(),
        timeout
    );
}

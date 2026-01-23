pub fn formatted_blocktorch_timeline(
    kernel_digest: Option<&str>,
    benchmark_commit: Option<&str>,
    proof_latency_ms: Option<f64>,
    aggregator_trace: Option<&str>,
) -> Option<Vec<String>> {
    if kernel_digest.is_none()
        && benchmark_commit.is_none()
        && proof_latency_ms.is_none()
        && aggregator_trace.is_none()
    {
        return None;
    }

    let mut lines = Vec::with_capacity(5);
    lines.push("BlockTorch job timeline:".to_string());
    if let Some(digest) = kernel_digest {
        lines.push(format!("  kernel digest: {digest}"));
    }
    if let Some(commit) = benchmark_commit {
        lines.push(format!("  benchmark commit: {commit}"));
    }
    if let Some(latency) = proof_latency_ms {
        lines.push(format!(
            "  proof latency (ms, last measurement): {latency:.2}"
        ));
    }
    if let Some(trace) = aggregator_trace {
        lines.push(format!("  aggregator trace: {trace}"));
    }

    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::formatted_blocktorch_timeline;

    #[test]
    fn formatted_blocktorch_timeline_emits_lines_in_order() {
        let lines = formatted_blocktorch_timeline(
            Some("digest-123"),
            Some("bench-abc"),
            Some(42.555),
            Some("trace-xyz"),
        )
        .expect("timeline should be present");
        let expected = vec![
            "BlockTorch job timeline:".to_string(),
            "  kernel digest: digest-123".to_string(),
            "  benchmark commit: bench-abc".to_string(),
            "  proof latency (ms, last measurement): 42.56".to_string(),
            "  aggregator trace: trace-xyz".to_string(),
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn formatted_blocktorch_timeline_is_empty_when_no_fields() {
        assert!(formatted_blocktorch_timeline(None, None, None, None).is_none());
    }
}

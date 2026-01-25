pub fn formatted_blocktorch_timeline(
    kernel_digest: Option<&str>,
    benchmark_commit: Option<&str>,
    tensor_profile_epoch: Option<&str>,
    descriptor_digest: Option<&str>,
    output_digest: Option<&str>,
    proof_latency_ms: Option<f64>,
    aggregator_trace: Option<&str>,
) -> Option<Vec<String>> {
    if kernel_digest.is_none()
        && benchmark_commit.is_none()
        && tensor_profile_epoch.is_none()
        && descriptor_digest.is_none()
        && output_digest.is_none()
        && proof_latency_ms.is_none()
        && aggregator_trace.is_none()
    {
        return None;
    }

    let mut lines = Vec::with_capacity(6);
    lines.push("BlockTorch job timeline:".to_string());
    if let Some(digest) = kernel_digest {
        lines.push(format!("  kernel digest: {digest}"));
    }
    if let Some(commit) = benchmark_commit {
        lines.push(format!("  benchmark commit: {commit}"));
    }
    if let Some(epoch) = tensor_profile_epoch {
        lines.push(format!("  tensor profile epoch: {epoch}"));
    }
    if let Some(descriptor) = descriptor_digest {
        lines.push(format!("  descriptor digest: {descriptor}"));
    }
    if let Some(output) = output_digest {
        lines.push(format!("  output digest: {output}"));
    }
    if let Some(latency) = proof_latency_ms {
        lines.push(format!(
            "  proof latency (ms, last measurement): {}",
            format_proof_latency(latency)
        ));
    }
    if let Some(trace) = aggregator_trace {
        lines.push(format!("  aggregator trace: {trace}"));
    }

    Some(lines)
}

fn format_proof_latency(latency: f64) -> String {
    let scaled = (latency * 1_000.0).round() as i64;
    let remainder = (scaled.abs() % 10) as i64;
    let base = scaled / 10;
    let adjusted = if remainder >= 5 {
        if scaled >= 0 {
            base + 1
        } else {
            base - 1
        }
    } else {
        base
    };
    let value = (adjusted as f64) / 100.0;
    format!("{value:.2}")
}

#[cfg(test)]
mod tests {
    use super::formatted_blocktorch_timeline;

    #[test]
    fn formatted_blocktorch_timeline_emits_lines_in_order() {
        let lines = formatted_blocktorch_timeline(
            Some("digest-123"),
            Some("bench-abc"),
            Some("epoch-99"),
            Some("descriptor-456"),
            Some("output-789"),
            Some(42.555),
            Some("trace-xyz"),
        )
        .expect("timeline should be present");
        let expected = vec![
            "BlockTorch job timeline:".to_string(),
            "  kernel digest: digest-123".to_string(),
            "  benchmark commit: bench-abc".to_string(),
            "  tensor profile epoch: epoch-99".to_string(),
            "  descriptor digest: descriptor-456".to_string(),
            "  output digest: output-789".to_string(),
            "  proof latency (ms, last measurement): 42.56".to_string(),
            "  aggregator trace: trace-xyz".to_string(),
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn formatted_blocktorch_timeline_is_empty_when_no_fields() {
        assert!(formatted_blocktorch_timeline(None, None, None, None, None, None, None).is_none());
    }
}

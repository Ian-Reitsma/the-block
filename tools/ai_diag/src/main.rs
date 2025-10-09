use anyhow::{anyhow, Result};
use std::env;

#[derive(Debug)]
struct Metrics {
    avg_latency_ms: u64,
}

fn suggest(metrics: &Metrics) -> Vec<String> {
    let mut out = Vec::new();
    if metrics.avg_latency_ms > 1_000 {
        out.push(format!(
            "high latency ({}) detected; consider increasing worker threads",
            metrics.avg_latency_ms
        ));
    }
    out
}

fn main() -> Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| "metrics.json".into());
    let data = std::fs::read_to_string(path)?;
    let metrics = parse_metrics(&data)?;
    for s in suggest(&metrics) {
        println!("{s}");
    }
    Ok(())
}

fn parse_metrics(data: &str) -> Result<Metrics> {
    let needle = "\"avg_latency_ms\"";
    let key_start = data
        .find(needle)
        .ok_or_else(|| anyhow!("metrics must contain an 'avg_latency_ms' field"))?;
    let after_key = &data[key_start + needle.len()..];
    let colon_index = after_key
        .find(':')
        .ok_or_else(|| anyhow!("missing ':' after 'avg_latency_ms'"))?;
    let after_colon = after_key[colon_index + 1..].trim_start();
    let digits_len = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits_len == 0 {
        return Err(anyhow!("'avg_latency_ms' must be an unsigned integer"));
    }
    let number_str = &after_colon[..digits_len];
    let avg_latency_ms = number_str
        .parse::<u64>()
        .map_err(|err| anyhow!("failed to parse 'avg_latency_ms': {err}"))?;
    Ok(Metrics { avg_latency_ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_whitespace() {
        let metrics = parse_metrics("{  \n  \"avg_latency_ms\"  :  1200  } ").expect("parse");
        assert_eq!(metrics.avg_latency_ms, 1200);
    }

    #[test]
    fn errors_when_field_missing() {
        let err = parse_metrics("{ \"other\": 1 }").expect_err("missing field should error");
        assert!(err.to_string().contains("avg_latency_ms"));
    }

    #[test]
    fn errors_on_invalid_numbers() {
        let err = parse_metrics("{\"avg_latency_ms\": \"fast\"}").expect_err("invalid value");
        assert!(err
            .to_string()
            .contains("'avg_latency_ms' must be an unsigned integer"));
    }
}

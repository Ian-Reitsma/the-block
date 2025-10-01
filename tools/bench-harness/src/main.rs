#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use coding::{compressor_for, erasure_coder_for};
use rand::{rngs::StdRng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Distributed benchmarking harness CLI.
#[derive(Debug, Parser)]
#[command(name = "bench-harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the placeholder workload benchmark.
    Workload {
        /// Number of nodes to deploy for the benchmark.
        #[arg(short, long, default_value_t = 1)]
        nodes: u32,
        /// Optional workload configuration in JSON format.
        #[arg(short, long)]
        workload: Option<PathBuf>,
        /// Path to write benchmark report JSON.
        #[arg(long)]
        report: PathBuf,
        /// Optional baseline metrics to detect regressions.
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
    /// Compare default and fallback coding stacks.
    CompareCoders {
        /// Payload size in bytes for each iteration.
        #[arg(long, default_value_t = 1_048_576)]
        bytes: usize,
        /// Number of data shards for erasure coding.
        #[arg(long, default_value_t = 16)]
        data: usize,
        /// Number of parity shards for erasure coding.
        #[arg(long, default_value_t = 1)]
        parity: usize,
        /// Number of iterations to average over.
        #[arg(long, default_value_t = 32)]
        iterations: u32,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Metrics {
    latency_ms: f64,
    throughput_tps: f64,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Workload {
            nodes,
            workload,
            report,
            baseline,
        } => run_workload_mode(nodes, workload, report, baseline),
        Command::CompareCoders {
            bytes,
            data,
            parity,
            iterations,
        } => compare_coders(bytes, data, parity, iterations),
    }
}

fn run_workload_mode(
    nodes: u32,
    workload: Option<PathBuf>,
    report: PathBuf,
    baseline: Option<PathBuf>,
) -> anyhow::Result<()> {
    deploy_nodes(nodes)?;
    let metrics = run_workload(workload)?;
    if let Some(base) = &baseline {
        regression_check(&metrics, base)?;
    }
    fs::write(&report, serde_json::to_vec_pretty(&metrics)?)?;
    Ok(())
}

fn deploy_nodes(nodes: u32) -> anyhow::Result<()> {
    for n in 0..nodes {
        println!("starting node {n}");
        // Placeholder: spawn containers or remote nodes.
    }
    Ok(())
}

fn run_workload(workload: Option<PathBuf>) -> anyhow::Result<Metrics> {
    if let Some(path) = workload {
        let _cfg: serde_json::Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    }
    // In lieu of real metrics, return deterministic placeholder values.
    Ok(Metrics {
        latency_ms: 10.0,
        throughput_tps: 1_000.0,
    })
}

fn regression_check(metrics: &Metrics, baseline_path: &PathBuf) -> anyhow::Result<()> {
    let data = fs::read_to_string(baseline_path)?;
    let baseline: Metrics = serde_json::from_str(&data)?;
    if metrics.throughput_tps < baseline.throughput_tps {
        eprintln!(
            "throughput regression: {} < {}",
            metrics.throughput_tps, baseline.throughput_tps
        );
    }
    Ok(())
}

fn compare_coders(bytes: usize, data: usize, parity: usize, iterations: u32) -> anyhow::Result<()> {
    if data == 0 {
        anyhow::bail!("data shards must be greater than zero");
    }
    let mut rng = StdRng::seed_from_u64(42);
    let mut payload = vec![0u8; bytes];
    rng.fill_bytes(&mut payload);

    let rs = erasure_coder_for("reed-solomon", data, parity)?;
    let xor = erasure_coder_for("xor", data, parity)?;
    let hybrid = compressor_for("lz77-rle", 4)?;
    let rle = compressor_for("rle", 0)?;

    let mut rs_encode = Duration::ZERO;
    let mut rs_decode = Duration::ZERO;
    let mut xor_encode = Duration::ZERO;
    let mut xor_decode = Duration::ZERO;
    let mut hybrid_compress = Duration::ZERO;
    let mut hybrid_decompress = Duration::ZERO;
    let mut hybrid_bytes = 0usize;
    let mut rle_compress = Duration::ZERO;
    let mut rle_decompress = Duration::ZERO;
    let mut rle_bytes = 0usize;

    for _ in 0..iterations {
        let start = Instant::now();
        let rs_batch = rs.encode(&payload)?;
        rs_encode += start.elapsed();
        let mut rs_slots = vec![None; rs_batch.shards.len()];
        for shard in rs_batch.shards.iter() {
            rs_slots[shard.index] = Some(shard.clone());
        }
        if rs_batch.metadata.parity_shards > 0 && !rs_slots.is_empty() {
            rs_slots[0] = None;
        }
        let start = Instant::now();
        let recovered = rs.reconstruct(&rs_batch.metadata, &rs_slots)?;
        rs_decode += start.elapsed();
        assert_eq!(recovered, payload);

        let start = Instant::now();
        let xor_batch = xor.encode(&payload)?;
        xor_encode += start.elapsed();
        let mut xor_slots = vec![None; xor_batch.shards.len()];
        for shard in xor_batch.shards.iter() {
            xor_slots[shard.index] = Some(shard.clone());
        }
        if xor_batch.metadata.parity_shards > 0 && !xor_slots.is_empty() {
            xor_slots[0] = None;
        }
        let start = Instant::now();
        let xor_recovered = xor.reconstruct(&xor_batch.metadata, &xor_slots)?;
        xor_decode += start.elapsed();
        assert_eq!(xor_recovered, payload);

        let start = Instant::now();
        let hybrid_buf = hybrid.compress(&payload)?;
        hybrid_compress += start.elapsed();
        hybrid_bytes += hybrid_buf.len();
        let start = Instant::now();
        let hybrid_plain = hybrid.decompress(&hybrid_buf)?;
        hybrid_decompress += start.elapsed();
        assert_eq!(hybrid_plain, payload);

        let start = Instant::now();
        let rle_buf = rle.compress(&payload)?;
        rle_compress += start.elapsed();
        rle_bytes += rle_buf.len();
        let start = Instant::now();
        let rle_plain = rle.decompress(&rle_buf)?;
        rle_decompress += start.elapsed();
        assert_eq!(rle_plain, payload);
    }

    let total_bytes = bytes as f64 * iterations as f64;

    println!("== Erasure coding benchmark ==");
    print_timing(
        "inhouse reed-solomon encode",
        rs_encode,
        iterations,
        total_bytes,
    );
    print_timing(
        "inhouse reed-solomon reconstruct",
        rs_decode,
        iterations,
        total_bytes,
    );
    print_timing("xor encode", xor_encode, iterations, total_bytes);
    print_timing("xor reconstruct", xor_decode, iterations, total_bytes);

    let hybrid_ratio = hybrid_bytes as f64 / (iterations as f64 * bytes as f64);
    let rle_ratio = rle_bytes as f64 / (iterations as f64 * bytes as f64);

    println!("\n== Compression benchmark ==");
    print_timing("hybrid compress", hybrid_compress, iterations, total_bytes);
    print_timing(
        "hybrid decompress",
        hybrid_decompress,
        iterations,
        total_bytes,
    );
    println!("hybrid average ratio: {:.3}", hybrid_ratio);
    print_timing("rle compress", rle_compress, iterations, total_bytes);
    print_timing("rle decompress", rle_decompress, iterations, total_bytes);
    println!("rle average ratio: {:.3}", rle_ratio);

    Ok(())
}

fn print_timing(label: &str, total: Duration, iterations: u32, bytes: f64) {
    if iterations == 0 {
        println!("{label}: no iterations");
        return;
    }
    let avg_secs = total.as_secs_f64() / iterations as f64;
    let throughput = if total.is_zero() {
        f64::INFINITY
    } else {
        bytes / total.as_secs_f64() / (1024.0 * 1024.0)
    };
    println!(
        "{label}: {:.3} ms avg ({:.2} MiB/s)",
        avg_secs * 1_000.0,
        throughput
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_regression() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let baseline = Metrics {
            latency_ms: 10.0,
            throughput_tps: 2_000.0,
        };
        fs::write(tmp.path(), serde_json::to_string(&baseline).unwrap()).unwrap();
        let metrics = Metrics {
            latency_ms: 10.0,
            throughput_tps: 1_000.0,
        };
        regression_check(&metrics, &tmp.path().into()).unwrap();
    }
}

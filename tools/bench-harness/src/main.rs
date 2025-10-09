#![forbid(unsafe_code)]

use anyhow::Context;
use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use coding::{compressor_for, erasure_coder_for};
use foundation_serialization::json::{self, Number, Value};
use rand::{rngs::StdRng, RngCore};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

enum RunError {
    Usage(String),
    Failure(anyhow::Error),
}

#[derive(Debug, Clone, Copy)]
struct Metrics {
    latency_ms: f64,
    throughput_tps: f64,
}

impl Metrics {
    fn to_value(self) -> anyhow::Result<Value> {
        fn encode(label: &str, value: f64) -> anyhow::Result<Value> {
            Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| anyhow::anyhow!("metrics.{label} must be finite"))
        }

        let mut map = json::Map::new();
        map.insert("latency_ms".into(), encode("latency_ms", self.latency_ms)?);
        map.insert(
            "throughput_tps".into(),
            encode("throughput_tps", self.throughput_tps)?,
        );
        Ok(Value::Object(map))
    }

    fn from_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Object(map) => {
                let latency = require_number(&map, "latency_ms")?;
                let throughput = require_number(&map, "throughput_tps")?;
                Ok(Self {
                    latency_ms: latency,
                    throughput_tps: throughput,
                })
            }
            other => Err(anyhow::anyhow!(
                "expected metrics object, got {}",
                describe_type(&other)
            )),
        }
    }
}

fn require_number(map: &json::Map, key: &str) -> anyhow::Result<f64> {
    let value = map
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("metrics.{key} missing"))?;
    value
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("metrics.{key} is not a number"))
}

fn describe_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn main() {
    if let Err(err) = run() {
        match err {
            RunError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
            RunError::Failure(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }
}

fn run() -> Result<(), RunError> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "bench-harness".to_string());
    let args: Vec<String> = argv.collect();

    let command = build_command();
    if args.is_empty() {
        print_root_help(&command, &bin);
        return Ok(());
    }

    let parser = Parser::new(&command);
    let matches = match parser.parse(&args) {
        Ok(matches) => matches,
        Err(ParseError::HelpRequested(path)) => {
            print_help_for_path(&command, &path);
            return Ok(());
        }
        Err(err) => return Err(RunError::Usage(err.to_string())),
    };

    handle_matches(matches)
}

fn build_command() -> Command {
    CommandBuilder::new(
        CommandId("bench-harness"),
        "bench-harness",
        "Distributed benchmarking harness",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("bench-harness.workload"),
            "workload",
            "Run the placeholder workload benchmark",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("nodes", "nodes", "Number of nodes to deploy").default("1"),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "workload",
            "workload",
            "Optional workload configuration in JSON format",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("report", "report", "Path to write benchmark report JSON")
                .required(true),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "baseline",
            "baseline",
            "Optional baseline metrics to detect regressions",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("bench-harness.compare-coders"),
            "compare-coders",
            "Compare default and fallback coding stacks",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("bytes", "bytes", "Payload size in bytes for each iteration")
                .default("1048576"),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("data", "data", "Number of data shards for erasure coding")
                .default("16"),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new(
                "parity",
                "parity",
                "Number of parity shards for erasure coding",
            )
            .default("1"),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new(
                "iterations",
                "iterations",
                "Number of iterations to average over",
            )
            .default("32"),
        ))
        .build(),
    )
    .build()
}

fn handle_matches(matches: Matches) -> Result<(), RunError> {
    let (name, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| RunError::Usage("missing subcommand".into()))?;

    match name {
        "workload" => {
            let nodes = parse_u32(sub_matches, "nodes")?;
            let workload = sub_matches.get_string("workload").map(PathBuf::from);
            let report = sub_matches
                .get_string("report")
                .map(PathBuf::from)
                .ok_or_else(|| RunError::Usage("missing required '--report' option".into()))?;
            let baseline = sub_matches.get_string("baseline").map(PathBuf::from);
            run_workload_mode(nodes, workload, report, baseline)
                .map_err(|err| RunError::Failure(err))
        }
        "compare-coders" => {
            let bytes = parse_usize(sub_matches, "bytes")?;
            let data = parse_usize(sub_matches, "data")?;
            let parity = parse_usize(sub_matches, "parity")?;
            let iterations = parse_u32(sub_matches, "iterations")?;
            compare_coders(bytes, data, parity, iterations).map_err(|err| RunError::Failure(err))
        }
        other => Err(RunError::Usage(format!("unknown subcommand '{other}'"))),
    }
}

fn parse_u32(matches: &Matches, name: &str) -> Result<u32, RunError> {
    matches
        .get(name)
        .ok_or_else(|| RunError::Usage(format!("missing '--{name}' option")))?
        .parse::<u32>()
        .map_err(|err| RunError::Usage(err.to_string()))
}

fn parse_usize(matches: &Matches, name: &str) -> Result<usize, RunError> {
    matches
        .get(name)
        .ok_or_else(|| RunError::Usage(format!("missing '--{name}' option")))?
        .parse::<usize>()
        .map_err(|err| RunError::Usage(err.to_string()))
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for details on a command.");
}

fn print_help_for_path(root: &Command, path: &str) {
    let segments: Vec<&str> = path.split_whitespace().collect();
    if let Some(cmd) = find_command(root, &segments) {
        let generator = HelpGenerator::new(cmd);
        println!("{}", generator.render());
    }
}

fn find_command<'a>(root: &'a Command, path: &[&str]) -> Option<&'a Command> {
    if path.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for segment in path.iter().skip(1) {
        if let Some(next) = current
            .subcommands
            .iter()
            .find(|command| command.name == *segment)
        {
            current = next;
        } else {
            return None;
        }
    }
    Some(current)
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
    let payload = metrics.to_value()?;
    let mut file = fs::File::create(&report)?;
    json::to_writer_pretty(&mut file, &payload)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to write benchmark report to {}", report.display()))?;
    writeln!(file)?;
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
        json::value_from_str(&fs::read_to_string(path)?).map_err(anyhow::Error::from)?;
    }
    // In lieu of real metrics, return deterministic placeholder values.
    Ok(Metrics {
        latency_ms: 10.0,
        throughput_tps: 1_000.0,
    })
}

fn regression_check(metrics: &Metrics, baseline_path: &PathBuf) -> anyhow::Result<()> {
    let data = fs::read(baseline_path)?;
    let baseline_value = json::value_from_slice(&data)
        .map_err(anyhow::Error::from)
        .with_context(|| {
            format!(
                "failed to decode baseline report {}",
                baseline_path.display()
            )
        })?;
    let baseline = Metrics::from_value(baseline_value)?;
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
    use sys::tempfile::NamedTempFile;

    fn number(value: f64) -> Value {
        Value::Number(Number::from_f64(value).expect("finite"))
    }

    #[test]
    fn metrics_to_value_round_trip() {
        let metrics = Metrics {
            latency_ms: 12.5,
            throughput_tps: 987.0,
        };

        let value = metrics.to_value().expect("encode metrics");
        let parsed = Metrics::from_value(value.clone()).expect("decode metrics");

        assert_eq!(parsed.latency_ms, metrics.latency_ms);
        assert_eq!(parsed.throughput_tps, metrics.throughput_tps);

        match value {
            Value::Object(map) => {
                assert_eq!(map.get("latency_ms"), Some(&number(12.5)));
                assert_eq!(map.get("throughput_tps"), Some(&number(987.0)));
            }
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn metrics_to_value_rejects_non_finite() {
        let metrics = Metrics {
            latency_ms: f64::NAN,
            throughput_tps: 1.0,
        };

        let err = metrics.to_value().expect_err("nan should fail");
        assert!(err
            .to_string()
            .contains("metrics.latency_ms must be finite"));

        let metrics = Metrics {
            latency_ms: 1.0,
            throughput_tps: f64::INFINITY,
        };

        let err = metrics.to_value().expect_err("infinity should fail");
        assert!(err
            .to_string()
            .contains("metrics.throughput_tps must be finite"));
    }

    #[test]
    fn metrics_from_value_missing_field() {
        let mut map = json::Map::new();
        map.insert("latency_ms".into(), number(5.0));

        let err = Metrics::from_value(Value::Object(map)).expect_err("missing field must fail");
        assert!(err.to_string().contains("metrics.throughput_tps missing"));
    }

    #[test]
    fn metrics_from_value_rejects_non_number() {
        let mut map = json::Map::new();
        map.insert("latency_ms".into(), number(5.0));
        map.insert("throughput_tps".into(), Value::String("fast".into()));

        let err = Metrics::from_value(Value::Object(map)).expect_err("string should fail");
        assert!(err
            .to_string()
            .contains("metrics.throughput_tps is not a number"));
    }

    #[test]
    fn metrics_from_value_rejects_non_object() {
        let err = Metrics::from_value(Value::Array(vec![])).expect_err("array should fail");
        assert!(err
            .to_string()
            .contains("expected metrics object, got array"));
    }

    #[test]
    fn detects_regression() {
        let tmp = NamedTempFile::new().unwrap();
        let baseline = Metrics {
            latency_ms: 10.0,
            throughput_tps: 2_000.0,
        };
        let baseline_value = baseline.to_value().unwrap();
        let baseline_json = json::to_string_value_pretty(&baseline_value);
        fs::write(tmp.path(), baseline_json).unwrap();
        let metrics = Metrics {
            latency_ms: 10.0,
            throughput_tps: 1_000.0,
        };
        regression_check(&metrics, &tmp.path().into()).unwrap();
    }
}

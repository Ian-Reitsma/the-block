#![cfg_attr(not(feature = "dependency-fault"), allow(dead_code))]

#[cfg(not(feature = "dependency-fault"))]
fn main() {
    eprintln!(
        "dependency_fault binary requires the `dependency-fault` feature; rerun with `--features dependency-fault`"
    );
}

#[cfg(feature = "dependency-fault")]
fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;
    use std::time::Duration;

    use cli_core::{
        arg::{ArgSpec, FlagSpec, OptionSpec},
        command::{Command, CommandBuilder, CommandId},
        help::HelpGenerator,
        parse::{Matches, ParseError, Parser},
    };
    use tb_sim::dependency_fault_harness::{
        run_simulation, BackendSelections, CodecBackendChoice, CodingBackendChoice,
        CryptoBackendChoice, FaultSpec, OverlayBackendChoice, RuntimeBackendChoice,
        SimulationRequest, StorageBackendChoice, TransportBackendChoice, OUTPUT_ROOT,
    };

    let command = build_command();
    let mut argv = std::env::args();
    let bin = argv
        .next()
        .unwrap_or_else(|| "dependency-fault".to_string());
    let args: Vec<String> = argv.collect();
    if args.is_empty() {
        print_root_help(&command, &bin);
        return Ok(());
    }

    let parser = Parser::new(&command);
    let matches = match parser.parse(&args) {
        Ok(matches) => matches,
        Err(ParseError::HelpRequested(_)) => {
            print_root_help(&command, &bin);
            return Ok(());
        }
        Err(err) => return Err(anyhow::anyhow!(err.to_string())),
    };

    let runtime = parse_choice(&matches, "runtime", RuntimeBackendChoice::Tokio)?;
    let transport = parse_choice(&matches, "transport", TransportBackendChoice::Quinn)?;
    let overlay = parse_choice(&matches, "overlay", OverlayBackendChoice::Inhouse)?;
    let storage = parse_choice(&matches, "storage", StorageBackendChoice::RocksDb)?;
    let coding = parse_choice(&matches, "coding", CodingBackendChoice::ReedSolomon)?;
    let crypto = parse_choice(&matches, "crypto", CryptoBackendChoice::Dalek)?;
    let codec = parse_choice(&matches, "codec", CodecBackendChoice::Bincode)?;

    let faults = matches
        .get_strings("fault")
        .into_iter()
        .map(|raw| {
            raw.parse::<FaultSpec>()
                .map_err(|err| anyhow::anyhow!("invalid --fault value '{raw}': {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let duration_secs = matches
        .get_string("duration-secs")
        .unwrap_or_else(|| "5".to_string())
        .parse::<u64>()
        .map_err(|err| anyhow::anyhow!("invalid --duration-secs: {err}"))?;
    let iterations = matches
        .get_string("iterations")
        .unwrap_or_else(|| "1".to_string())
        .parse::<u32>()
        .map_err(|err| anyhow::anyhow!("invalid --iterations: {err}"))?;
    let label = matches.get_string("label");
    let output_dir = matches
        .get_string("output-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| OUTPUT_ROOT.clone());
    let persist_logs = !matches.get_flag("no-logs");

    let selections = BackendSelections {
        runtime,
        transport,
        overlay,
        storage,
        coding,
        crypto,
        codec,
    };
    let request = SimulationRequest {
        selections,
        faults,
        duration: Duration::from_secs(duration_secs.max(1)),
        iterations: iterations.max(1),
        output_root: output_dir,
        label,
        persist_logs,
    };

    let summary = run_simulation(&request)?;
    println!(
        "simulation artifacts stored under {}",
        summary.base_dir.display()
    );
    for report in summary.reports.iter() {
        println!(
            "- {} iteration {} => metrics: {}, summary: {}",
            report.metrics.scenario,
            report.metrics.iteration,
            report.metrics_path.display(),
            report.summary_path.display()
        );
    }
    Ok(())
}

#[cfg(feature = "dependency-fault")]
fn build_command() -> Command {
    CommandBuilder::new(
        CommandId("dependency_fault"),
        "dependency-fault",
        "Simulate dependency faults across wrapper backends",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("runtime", "runtime", "Runtime backend to use")
            .default(RuntimeBackendChoice::Tokio.as_str())
            .value_enum(RuntimeBackendChoice::variants()),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("transport", "transport", "Transport backend")
            .default(TransportBackendChoice::Quinn.as_str())
            .value_enum(TransportBackendChoice::variants()),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("overlay", "overlay", "Overlay backend")
            .default(OverlayBackendChoice::Inhouse.as_str())
            .value_enum(OverlayBackendChoice::variants()),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("storage", "storage", "Storage backend")
            .default(StorageBackendChoice::RocksDb.as_str())
            .value_enum(StorageBackendChoice::variants()),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("coding", "coding", "Coding backend")
            .default(CodingBackendChoice::ReedSolomon.as_str())
            .value_enum(CodingBackendChoice::variants()),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("crypto", "crypto", "Crypto backend")
            .default(CryptoBackendChoice::Dalek.as_str())
            .value_enum(CryptoBackendChoice::variants()),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("codec", "codec", "Codec backend")
            .default(CodecBackendChoice::Bincode.as_str())
            .value_enum(CodecBackendChoice::variants()),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "fault",
            "fault",
            "Fault specification in TARGET:KIND form (can be repeated)",
        )
        .multiple(true),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "duration-secs",
            "duration-secs",
            "Duration of each simulation run in seconds",
        )
        .default("5"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("iterations", "iterations", "Number of iterations to run").default("1"),
    ))
    .arg(ArgSpec::Option(OptionSpec::new(
        "label",
        "label",
        "Optional label recorded alongside artifacts",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "output-dir",
        "output-dir",
        "Directory for simulation artifacts",
    )))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "no-logs",
        "no-logs",
        "Skip persisting harness logs",
    )))
    .build()
}

#[cfg(feature = "dependency-fault")]
fn parse_choice<T>(matches: &Matches, key: &str, default: T) -> anyhow::Result<T>
where
    T: Copy + std::str::FromStr<Err = String>,
{
    match matches.get_string(key) {
        Some(raw) => raw
            .parse::<T>()
            .map_err(|err| anyhow::anyhow!("invalid --{key}: {err}")),
        None => Ok(default),
    }
}

#[cfg(feature = "dependency-fault")]
fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} --help' for usage.");
}

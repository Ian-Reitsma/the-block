use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use diagnostics::{anyhow, bail, Context, Result, TbError};
use foundation_serialization::json;
use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::Command as StdCommand,
};

#[derive(Default)]
struct Summary {
    balance_changed: bool,
    pending_changed: bool,
    title_ok: bool,
}

impl Summary {
    fn to_json_value(&self) -> json::Value {
        let mut map = json::Map::new();
        map.insert(
            "balance_changed".to_string(),
            json::Value::Bool(self.balance_changed),
        );
        map.insert(
            "pending_changed".to_string(),
            json::Value::Bool(self.pending_changed),
        );
        map.insert("title_ok".to_string(), json::Value::Bool(self.title_ok));
        json::Value::Object(map)
    }

    fn write_pretty_json(&self, mut writer: impl Write) -> Result<()> {
        let value = self.to_json_value();
        json::to_writer_pretty(&mut writer, &value)
            .map_err(|err| anyhow!(err))
            .context("failed to write summary output")
    }
}

fn main() -> Result<()> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "xtask".to_string());
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
        Err(err) => return Err(TbError::from_error(err)),
    };

    match matches.subcommand() {
        Some(("summary", sub_matches)) => run_summary(sub_matches),
        Some(("check-deps", sub_matches)) => run_check_deps(sub_matches),
        None => {
            print_root_help(&command, &bin);
            Ok(())
        }
        Some((other, _)) => bail!("unknown subcommand: {other}"),
    }
}

fn build_command() -> Command {
    CommandBuilder::new(CommandId("xtask"), "xtask", "Repository automation tasks")
        .subcommand(
            CommandBuilder::new(
                CommandId("xtask.summary"),
                "summary",
                "Summarise diff state",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("base", "base", "Base branch to diff against")
                    .default("origin/main"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "title",
                "title",
                "Proposed PR title",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("xtask.check_deps"),
                "check-deps",
                "Run the dependency registry check with first-party guard outputs",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "manifest-out",
                    "manifest-out",
                    "Path to write the crate manifest used by build guards",
                )
                .default("config/first_party_manifest.txt"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "out-dir",
                    "out-dir",
                    "Output directory for registry artefacts",
                )
                .default("target/dependency-registry"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "config",
                "config",
                "Dependency policy configuration path override",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "baseline",
                "baseline",
                "Baseline registry snapshot used for drift detection",
            )))
            .build(),
        )
        .build()
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} --help' for more details.");
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

fn run_summary(matches: &cli_core::parse::Matches) -> Result<()> {
    let base_ref = matches
        .get_string("base")
        .unwrap_or_else(|| "origin/main".to_string());
    let title = matches.get_string("title");

    let mut summary = Summary::default();
    let diff_output = git_diff(&base_ref)?;
    for line in diff_output.lines() {
        if line.contains("balance") {
            summary.balance_changed = true;
        }
        if line.contains("pending_") {
            summary.pending_changed = true;
        }
        if summary.balance_changed && summary.pending_changed {
            break;
        }
    }

    if let Some(t) = title {
        if summary.balance_changed || summary.pending_changed {
            summary.title_ok = t.starts_with("[core]");
        } else {
            summary.title_ok = true;
        }
    }

    summary.write_pretty_json(io::stdout())?;
    if !summary.title_ok {
        bail!("PR title does not match modified areas");
    }
    Ok(())
}

fn git_diff(base_ref: &str) -> Result<String> {
    let status = StdCommand::new("git")
        .args(["rev-parse", &format!("{base_ref}^{{commit}}")])
        .status()
        .context("failed to resolve base revision")?;
    if !status.success() {
        bail!("failed to resolve base revision: {base_ref}");
    }

    let diff_range = format!("{base_ref}..HEAD");
    let output = StdCommand::new("git")
        .args(["diff", "--patch", &diff_range])
        .output()
        .context("failed to execute git diff")?;
    if !output.status.success() {
        bail!("git diff exited with status {}", output.status);
    }
    String::from_utf8(output.stdout).context("git diff output was not valid UTF-8")
}

fn run_check_deps(matches: &cli_core::parse::Matches) -> Result<()> {
    let manifest_path = matches
        .get_string("manifest-out")
        .unwrap_or_else(|| "config/first_party_manifest.txt".to_string());
    let out_dir = matches
        .get_string("out-dir")
        .unwrap_or_else(|| "target/dependency-registry".to_string());
    let config = matches.get_string("config");
    let baseline = matches.get_string("baseline");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = StdCommand::new(cargo);
    command
        .arg("run")
        .arg("-p")
        .arg("dependency_registry")
        .arg("--");
    command
        .arg("--manifest-out")
        .arg(&manifest_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--check");

    if let Some(config_path) = config {
        command.arg("--config").arg(config_path);
    }
    if let Some(baseline_path) = baseline {
        command.arg("--baseline").arg(baseline_path);
    }

    let status = command
        .status()
        .context("failed to execute dependency-registry")?;
    if !status.success() {
        bail!("dependency registry check failed with status {status}");
    }

    let out_dir_path = Path::new(&out_dir);
    let summary_path = out_dir_path.join("dependency-check.summary.json");
    let summary_bytes = fs::read(&summary_path)
        .with_context(|| format!("failed to read {}", summary_path.display()))?;
    let summary_value = json::value_from_slice(&summary_bytes)
        .context("failed to parse dependency-check.summary.json")?;
    let status_label = summary_value
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let detail = summary_value
        .get("detail")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    println!("dependency registry check status: {status_label} ({detail})",);

    if let Some(counts) = summary_value
        .get("counts")
        .and_then(|value| value.as_object())
    {
        if !counts.is_empty() {
            let mut kinds: Vec<_> = counts.keys().cloned().collect();
            kinds.sort();
            println!("dependency registry drift counters:");
            for kind in kinds {
                let rendered = counts
                    .get(&kind)
                    .and_then(|value| value.as_i64().map(|v| v.to_string()))
                    .or_else(|| {
                        counts
                            .get(&kind)
                            .and_then(|value| value.as_u64().map(|v| v.to_string()))
                    })
                    .unwrap_or_else(|| counts.get(&kind).unwrap().to_string());
                println!("  {kind}: {rendered}");
            }
        }
    }
    Ok(())
}

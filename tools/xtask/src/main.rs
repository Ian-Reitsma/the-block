use anyhow::{anyhow, bail, Context, Result};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use foundation_serialization::json;
use git2::{DiffFormat, Repository};
use std::io::{self, Write};
use std::process::Command as StdCommand;

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
        Err(err) => return Err(err.into()),
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
            .arg(ArgSpec::Flag(FlagSpec::new(
                "allow-third-party",
                "allow-third-party",
                "Temporarily disable the FIRST_PARTY_ONLY guard while running the check",
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

    let repo = Repository::discover(".")?;
    let base = repo.revparse_single(&base_ref)?.peel_to_commit()?;
    let head = repo.head()?.peel_to_commit()?;
    let base_tree = base.tree()?;
    let head_tree = head.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)?;

    let mut summary = Summary::default();
    diff.print(DiffFormat::Patch, |_, _, line| {
        if let Ok(content) = std::str::from_utf8(line.content()) {
            if content.contains("balance") {
                summary.balance_changed = true;
            }
            if content.contains("pending_") {
                summary.pending_changed = true;
            }
        }
        true
    })?;

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

fn run_check_deps(matches: &cli_core::parse::Matches) -> Result<()> {
    let manifest_path = matches
        .get_string("manifest-out")
        .unwrap_or_else(|| "config/first_party_manifest.txt".to_string());
    let out_dir = matches
        .get_string("out-dir")
        .unwrap_or_else(|| "target/dependency-registry".to_string());
    let config = matches.get_string("config");
    let baseline = matches.get_string("baseline");
    let allow_third_party = matches.get_flag("allow-third-party");

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

    if allow_third_party {
        command.env("FIRST_PARTY_ONLY", "0");
    }

    let status = command
        .status()
        .context("failed to execute dependency-registry")?;
    if !status.success() {
        bail!("dependency registry check failed with status {status}");
    }
    Ok(())
}

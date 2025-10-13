use std::path::PathBuf;

use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use diagnostics::anyhow::{anyhow, Result};

use release_notes::{
    format_allowed, format_change_summary, kind_label, load_history, summarise, summary_to_value,
    Filter, KNOWN_KINDS,
};

#[derive(Debug)]
struct Cli {
    /// Directory containing governance state (expects governance/history/dependency_policy.json).
    state_dir: Option<PathBuf>,

    /// Explicit path to the dependency policy history JSON file.
    history: Option<PathBuf>,

    /// Only include records with epoch >= this value.
    since_epoch: Option<u64>,

    /// Only include records with proposal_id >= this value.
    since_proposal: Option<u64>,

    /// Render the summary as JSON instead of human readable text.
    json: bool,
}

impl Cli {
    fn build_command() -> Command {
        CommandBuilder::new(
            CommandId("release-notes"),
            "release-notes",
            "Generate release note snippets for governance dependency policy changes",
        )
        .arg(ArgSpec::Option(OptionSpec::new(
            "state-dir",
            "state-dir",
            "Directory containing governance state",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "history",
            "history",
            "Explicit path to the dependency policy history JSON file",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "since-epoch",
            "since-epoch",
            "Only include records with epoch >= this value",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "since-proposal",
            "since-proposal",
            "Only include records with proposal_id >= this value",
        )))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "json",
            "json",
            "Emit the summary as JSON instead of human readable text",
        )))
        .build()
    }

    fn from_matches(matches: &Matches) -> Result<Self> {
        let state_dir = matches.get_string("state-dir").map(PathBuf::from);
        let history = matches.get_string("history").map(PathBuf::from);
        let since_epoch = matches
            .get("since-epoch")
            .map(|value| value.parse::<u64>().map_err(|err| anyhow!(err)))
            .transpose()?;
        let since_proposal = matches
            .get("since-proposal")
            .map(|value| value.parse::<u64>().map_err(|err| anyhow!(err)))
            .transpose()?;

        Ok(Self {
            state_dir,
            history,
            since_epoch,
            since_proposal,
            json: matches.get_flag("json"),
        })
    }

    fn history_path(&self) -> PathBuf {
        if let Some(explicit) = &self.history {
            return explicit.clone();
        }
        if let Some(state) = &self.state_dir {
            return state
                .join("governance")
                .join("history")
                .join("dependency_policy.json");
        }
        PathBuf::from("governance/history/dependency_policy.json")
    }
}

fn main() -> Result<()> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "release-notes".to_string());
    let args: Vec<String> = argv.collect();

    let command = Cli::build_command();
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
        Err(err) => return Err(anyhow!(err)),
    };

    let cli = Cli::from_matches(&matches)?;
    let history_path = cli.history_path();
    let records = load_history(&history_path)?;

    let summary = summarise(
        &records,
        Filter {
            since_epoch: cli.since_epoch,
            since_proposal: cli.since_proposal,
        },
    );

    if cli.json {
        let rendered = json::to_string_value_pretty(&summary_to_value(&summary));
        println!("{rendered}");
        return Ok(());
    }

    println!("## Governance Dependency Policy Updates\n");
    if summary.updates.is_empty() {
        println!(
            "No governance-approved dependency backend changes during this window ({}).\n",
            history_path.display()
        );
    } else {
        println!(
            "Governance approved the following dependency backend updates (source: {}).\n",
            history_path.display()
        );
        for update in &summary.updates {
            let label = kind_label(&update.kind);
            let delta_note = format_change_summary(&update.added, &update.removed);
            match &update.previous {
                Some(previous) => {
                    let change = format!(
                        "{} -> {}",
                        format_allowed(previous),
                        format_allowed(&update.current)
                    );
                    if let Some(note) = delta_note {
                        println!(
                            "- {label} policy updated in epoch {} (proposal #{}): {change} ({note}).",
                            update.epoch,
                            update.proposal_id,
                        );
                    } else {
                        println!(
                            "- {label} policy updated in epoch {} (proposal #{}): {change}.",
                            update.epoch, update.proposal_id,
                        );
                    }
                }
                None => {
                    if let Some(note) = delta_note {
                        println!(
                            "- {label} policy initialised in epoch {} (proposal #{}): allowed backends {} ({note}).",
                            update.epoch,
                            update.proposal_id,
                            format_allowed(&update.current)
                        );
                    } else {
                        println!(
                            "- {label} policy initialised in epoch {} (proposal #{}): allowed backends {}.",
                            update.epoch,
                            update.proposal_id,
                            format_allowed(&update.current)
                        );
                    }
                }
            }
        }
        println!();
    }

    if summary.latest.is_empty() {
        println!(
            "No dependency policy history found at {}.",
            history_path.display()
        );
    } else {
        println!("Active dependency policies:\n");
        for kind in KNOWN_KINDS {
            if let Some(policy) = summary.latest.get(kind) {
                println!(
                    "- {} (epoch {}, proposal #{}): {}.",
                    kind_label(kind),
                    policy.epoch,
                    policy.proposal_id,
                    format_allowed(&policy.allowed)
                );
            }
        }
        for (kind, policy) in &summary.latest {
            if !KNOWN_KINDS.contains(&kind.as_str()) {
                println!(
                    "- {} (epoch {}, proposal #{}): {}.",
                    kind,
                    policy.epoch,
                    policy.proposal_id,
                    format_allowed(&policy.allowed)
                );
            }
        }
    }

    Ok(())
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} --help' for more information.");
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

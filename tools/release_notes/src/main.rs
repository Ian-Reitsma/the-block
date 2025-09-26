use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use release_notes::{
    format_allowed, format_change_summary, kind_label, load_history, summarise, Filter, KNOWN_KINDS,
};

#[derive(Debug, Parser)]
#[command(
    name = "release-notes",
    about = "Generate release note snippets for governance dependency policy changes",
    version
)]
struct Cli {
    /// Directory containing governance state (expects governance/history/dependency_policy.json).
    #[arg(long, value_name = "PATH")]
    state_dir: Option<PathBuf>,

    /// Explicit path to the dependency policy history JSON file.
    #[arg(long, value_name = "PATH")]
    history: Option<PathBuf>,

    /// Only include records with epoch >= this value.
    #[arg(long, value_name = "EPOCH")]
    since_epoch: Option<u64>,

    /// Only include records with proposal_id >= this value.
    #[arg(long, value_name = "ID")]
    since_proposal: Option<u64>,
}

impl Cli {
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
    let cli = Cli::parse();
    let history_path = cli.history_path();
    let records = load_history(&history_path)?;

    let summary = summarise(
        &records,
        Filter {
            since_epoch: cli.since_epoch,
            since_proposal: cli.since_proposal,
        },
    );

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

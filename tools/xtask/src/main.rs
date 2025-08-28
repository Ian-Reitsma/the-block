use anyhow::{bail, Result};
use clap::Parser;
use git2::{DiffFormat, Repository};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[clap(long, default_value = "origin/main")]
    base: String,
    #[clap(long)]
    title: Option<String>,
}

#[derive(Serialize, Default)]
struct Summary {
    balance_changed: bool,
    pending_changed: bool,
    title_ok: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let repo = Repository::discover(".")?;
    let base = repo.revparse_single(&args.base)?.peel_to_commit()?;
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

    if let Some(t) = args.title {
        if summary.balance_changed || summary.pending_changed {
            summary.title_ok = t.starts_with("[core]");
        } else {
            summary.title_ok = true;
        }
    }

    serde_json::to_writer_pretty(std::io::stdout(), &summary)?;
    if !summary.title_ok {
        bail!("PR title does not match modified areas");
    }
    Ok(())
}

use anyhow::{bail, Result};
use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use git2::{DiffFormat, Repository};
use serde::Serialize;

#[derive(Serialize, Default)]
struct Summary {
    balance_changed: bool,
    pending_changed: bool,
    title_ok: bool,
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

    serde_json::to_writer_pretty(std::io::stdout(), &summary)?;
    if !summary.title_ok {
        bail!("PR title does not match modified areas");
    }
    Ok(())
}

fn build_command() -> Command {
    CommandBuilder::new(CommandId("xtask"), "xtask", "Repository automation tasks")
        .arg(ArgSpec::Option(
            OptionSpec::new("base", "base", "Base branch to diff against").default("origin/main"),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "title",
            "title",
            "Proposed PR title",
        )))
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

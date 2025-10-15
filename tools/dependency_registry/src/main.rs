use std::path::PathBuf;

use cli_core::{
    command::Command,
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use diagnostics::anyhow as diag_anyhow;
use diagnostics::anyhow::Result;
use std::env;

use dependency_registry::{output, run_cli, Cli};

fn main() -> Result<()> {
    let mut argv = env::args();
    let bin = argv
        .next()
        .unwrap_or_else(|| "dependency-registry".to_string());
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
        Err(err) => return Err(diag_anyhow::anyhow!(err)),
    };

    let cli = Cli::from_matches(&matches)?;

    if let Some(diff) = &cli.diff {
        let paths: Vec<PathBuf> = diff.clone();
        output::diff_registries(&paths[0], &paths[1])?;
        return Ok(());
    }

    if let Some(crate_name) = &cli.explain {
        output::explain_crate(crate_name, &cli.baseline)?;
        return Ok(());
    }

    run_cli(&cli).map(|_| ())
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} --help' or '{bin} help <section>' for more information.");
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

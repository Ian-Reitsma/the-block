use std::{env, process};

use cli_core::{
    command::Command as CliCommand,
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};

pub fn collect_args(default_bin: &str) -> (String, Vec<String>) {
    let mut argv = env::args();
    let bin = argv.next().unwrap_or_else(|| default_bin.to_string());
    let args: Vec<String> = argv.collect();
    (bin, args)
}

pub fn parse_matches(command: &CliCommand, bin: &str, args: Vec<String>) -> Option<Matches> {
    if args.is_empty() {
        print_root_help(command, bin);
        return None;
    }

    let parser = Parser::new(command);
    match parser.parse(&args) {
        Ok(matches) => Some(matches),
        Err(ParseError::HelpRequested(path)) => {
            print_help_for_path(command, &path);
            None
        }
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    }
}

pub fn print_root_help(command: &CliCommand, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for details on a command.");
}

pub fn print_help_for_path(root: &CliCommand, path: &str) {
    let segments: Vec<&str> = path.split_whitespace().collect();
    if let Some(cmd) = find_command(root, &segments) {
        let generator = HelpGenerator::new(cmd);
        println!("{}", generator.render());
    }
}

fn find_command<'a>(root: &'a CliCommand, segments: &[&str]) -> Option<&'a CliCommand> {
    if segments.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for segment in segments.iter().skip(1) {
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

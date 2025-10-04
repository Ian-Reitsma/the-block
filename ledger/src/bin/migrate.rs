use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use ledger::utxo_account::migrate_accounts;
use serde_json;
use std::{collections::HashMap, fs};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "migrate".to_string());
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
        Err(err) => return Err(err.to_string()),
    };

    let input = matches
        .get_string("input")
        .ok_or_else(|| "missing required '--input' option".to_string())?;
    let output = matches
        .get_string("output")
        .ok_or_else(|| "missing required '--output' option".to_string())?;

    let data = fs::read_to_string(&input).map_err(|err| err.to_string())?;
    let balances: HashMap<String, u64> = serde_json::from_str(&data).expect("parse input");
    let utxo = migrate_accounts(&balances);
    let out = serde_json::to_string_pretty(&utxo).expect("serialize");
    fs::write(&output, out).map_err(|err| err.to_string())?;
    Ok(())
}

fn build_command() -> Command {
    CommandBuilder::new(
        CommandId("ledger-migrate"),
        "ledger-migrate",
        "Migrate account balances to the UTXO representation",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("input", "input", "Input balance JSON file").required(true),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("output", "output", "Output UTXO JSON file").required(true),
    ))
    .build()
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} --help' for details.");
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

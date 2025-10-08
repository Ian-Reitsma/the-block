use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use foundation_serialization::json::{self, Map, Number, Value};
use ledger::utxo_account::{migrate_accounts, UtxoLedger};
use std::{collections::HashMap, fs, path::Path};

#[derive(Debug)]
struct MigrateError(String);

impl From<String> for MigrateError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MigrateError {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), MigrateError> {
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
        Err(err) => return Err(MigrateError(err.to_string())),
    };

    let input = matches
        .get_string("input")
        .ok_or_else(|| "missing required '--input' option".to_string())?;
    let output = matches
        .get_string("output")
        .ok_or_else(|| "missing required '--output' option".to_string())?;

    let balances = read_balances(&input)?;
    let utxo = migrate_accounts(&balances);
    write_utxo(&output, &utxo)?;
    Ok(())
}

fn read_balances(path: &str) -> Result<HashMap<String, u64>, MigrateError> {
    let raw = read_file(path)?;
    json::from_str(&raw)
        .map_err(|err| MigrateError(format!("failed to parse balances from '{}': {err}", path)))
}

fn write_utxo(path: &str, utxo: &UtxoLedger) -> Result<(), MigrateError> {
    let mut entries: Vec<_> = utxo.utxos.iter().collect();
    entries.sort_by(|a, b| {
        let order = a.0.txid.cmp(&b.0.txid);
        if order == std::cmp::Ordering::Equal {
            a.0.index.cmp(&b.0.index)
        } else {
            order
        }
    });

    let mut array = Vec::with_capacity(entries.len());
    for (point, entry) in entries {
        let mut obj = Map::new();
        obj.insert("txid".to_string(), Value::String(hex_encode(&point.txid)));
        obj.insert(
            "index".to_string(),
            Value::Number(Number::from(point.index)),
        );
        obj.insert("owner".to_string(), Value::String(entry.owner.clone()));
        obj.insert(
            "value".to_string(),
            Value::Number(Number::from(entry.value)),
        );
        array.push(Value::Object(obj));
    }

    let mut encoded = json::to_string_value(&Value::Array(array));
    if !encoded.ends_with('\n') {
        encoded.push('\n');
    }
    fs::write(path, encoded).map_err(|err| {
        MigrateError(format!(
            "failed to write migrated balances to '{}': {err}",
            path
        ))
    })
}

fn read_file(path: &str) -> Result<String, MigrateError> {
    let path_ref = Path::new(path);
    fs::read_to_string(path_ref).map_err(|err| {
        if path_ref.exists() {
            MigrateError(format!("failed to read '{}': {err}", path))
        } else {
            MigrateError(format!("input file '{}' does not exist", path))
        }
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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

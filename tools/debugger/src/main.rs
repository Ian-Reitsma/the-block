use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use the_block::SimpleDb;

enum RunError {
    Usage(String),
}

fn main() {
    if let Err(err) = run() {
        match err {
            RunError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
        }
    }
}

fn run() -> Result<(), RunError> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "debugger".to_string());
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
        Err(err) => return Err(RunError::Usage(err.to_string())),
    };

    let db_path = matches
        .get_string("db")
        .unwrap_or_else(|| "node-data".to_string());
    let db = SimpleDb::open(&db_path);

    let (name, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| RunError::Usage("missing subcommand".into()))?;

    match name {
        "get" => {
            let key = positional(sub_matches, "key")?;
            if let Some(v) = db.get(&key) {
                println!("{}", hex::encode(v));
            } else {
                eprintln!("key not found");
            }
        }
        "keys" => {
            let prefix = positional(sub_matches, "prefix")?;
            for k in db.keys_with_prefix(&prefix) {
                println!("{k}");
            }
        }
        other => return Err(RunError::Usage(format!("unknown subcommand '{other}'"))),
    }

    Ok(())
}

fn build_command() -> Command {
    CommandBuilder::new(
        CommandId("debugger"),
        "debugger",
        "Inspect node state and transactions",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("db", "db", "Path to the node database").default("node-data"),
    ))
    .subcommand(
        CommandBuilder::new(CommandId("debugger.get"), "get", "Fetch a value by key")
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "key",
                "Key to fetch",
            )))
            .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("debugger.keys"),
            "keys",
            "List keys with a prefix",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "prefix",
            "Prefix to filter keys",
        )))
        .build(),
    )
    .build()
}

fn positional(matches: &Matches, name: &str) -> Result<String, RunError> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| RunError::Usage(format!("missing '{name}' argument")))
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for more details.");
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

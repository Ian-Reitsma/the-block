use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use httpd::{BlockingClient, Method};
use serde::Deserialize;
use foundation_serialization::json;

#[derive(Deserialize, Debug)]
struct Summary {
    epoch: u64,
    receipts: u64,
    invalid: u64,
}

fn main() {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "audit".to_string());
    let args: Vec<String> = argv.collect();

    let command = build_command();
    if args.is_empty() {
        print_root_help(&command, &bin);
        return;
    }

    let parser = Parser::new(&command);
    let matches = match parser.parse(&args) {
        Ok(matches) => matches,
        Err(ParseError::HelpRequested(path)) => {
            print_help_for_path(&command, &path);
            return;
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    let rpc = matches
        .get_string("rpc")
        .unwrap_or_else(|| "http://127.0.0.1:8545".to_string());

    let body = foundation_serialization::json!({"method":"settlement.audit"});
    let res: json::Value = BlockingClient::default()
        .request(Method::Post, &rpc)
        .and_then(|builder| builder.json(&body))
        .and_then(|builder| builder.send())
        .expect("rpc")
        .json()
        .expect("json");
    let list: Vec<Summary> = json::from_value(res["result"].clone()).unwrap_or_default();
    for s in list {
        println!("epoch {} receipts {} invalid {}", s.epoch, s.receipts, s.invalid);
    }
}

fn build_command() -> Command {
    CommandBuilder::new(
        CommandId("audit"),
        "audit",
        "Fetch settlement audit summaries",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("rpc", "rpc", "RPC endpoint").default("http://127.0.0.1:8545"),
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

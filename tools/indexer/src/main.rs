use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use httpd::{serve, serve_tls, ServerConfig, ServerTlsConfig};
use indexer::{router, BlockRecord, Indexer};
use runtime::net::TcpListener;
use serde_json;
use std::{env, fs::File, net::SocketAddr, path::Path, process};

enum RunError {
    Usage(String),
    Failure(anyhow::Error),
}

fn main() {
    if let Err(err) = run() {
        match err {
            RunError::Usage(msg) => {
                eprintln!("{msg}");
                process::exit(2);
            }
            RunError::Failure(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
        }
    }
}

fn run() -> Result<(), RunError> {
    let mut argv = env::args();
    let bin = argv.next().unwrap_or_else(|| "indexer".to_string());
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

    handle_matches(matches)
}

fn build_command() -> Command {
    CommandBuilder::new(CommandId("indexer"), "indexer", "Indexer database tooling")
        .subcommand(
            CommandBuilder::new(
                CommandId("indexer.index"),
                "index",
                "Index blocks from a JSON file",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "file",
                "Path to JSON block records",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "SQLite database path",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("indexer.serve"),
                "serve",
                "Serve an HTTP view over the indexed database",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "SQLite database path",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("addr", "addr", "Address to bind").default("0.0.0.0:3000"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("indexer.profile"),
                "profile",
                "Print basic statistics from the database",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "SQLite database path",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("indexer.index_receipts"),
                "index-receipts",
                "Index checkpointed receipts from a directory",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "dir",
                "Directory containing receipt snapshots",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "db",
                "SQLite database path",
            )))
            .build(),
        )
        .build()
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for details on a command.");
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

fn handle_matches(matches: Matches) -> Result<(), RunError> {
    let (name, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| RunError::Usage("missing subcommand".into()))?;

    match name {
        "index" => {
            let file = positional(sub_matches, "file")?;
            let db = positional(sub_matches, "db")?;
            let idx = Indexer::open(&db).map_err(|err| RunError::Failure(err.into()))?;
            let reader = File::open(&file).map_err(|err| RunError::Failure(err.into()))?;
            let records: Vec<BlockRecord> =
                serde_json::from_reader(reader).map_err(|err| RunError::Failure(err.into()))?;
            for record in &records {
                idx.index_block(record)
                    .map_err(|err| RunError::Failure(err.into()))?;
            }
        }
        "serve" => {
            let db = positional(sub_matches, "db")?;
            let addr = sub_matches
                .get_string("addr")
                .unwrap_or_else(|| "0.0.0.0:3000".to_string());
            let addr: SocketAddr = addr
                .parse()
                .map_err(|err| RunError::Usage(format!("invalid value for '--addr': {err}")))?;
            let idx = Indexer::open(&db).map_err(|err| RunError::Failure(err.into()))?;
            runtime::block_on(async move {
                let listener = TcpListener::bind(addr)
                    .await
                    .map_err(|err| RunError::Failure(err.into()))?;
                let config = ServerConfig::default();
                let app = router(idx);
                if let (Ok(cert), Ok(key)) = (env::var("INDEXER_CERT"), env::var("INDEXER_KEY")) {
                    let tls = if let Ok(ca) = env::var("INDEXER_CLIENT_CA") {
                        ServerTlsConfig::from_pem_files_with_client_auth(cert, key, ca)
                            .map_err(|err| RunError::Failure(err.into()))?
                    } else if let Ok(ca) = env::var("INDEXER_CLIENT_CA_OPTIONAL") {
                        ServerTlsConfig::from_pem_files_with_optional_client_auth(cert, key, ca)
                            .map_err(|err| RunError::Failure(err.into()))?
                    } else {
                        ServerTlsConfig::from_pem_files(cert, key)
                            .map_err(|err| RunError::Failure(err.into()))?
                    };
                    serve_tls(listener, app, config, tls)
                        .await
                        .map_err(|err| RunError::Failure(err.into()))?;
                } else {
                    serve(listener, app, config)
                        .await
                        .map_err(|err| RunError::Failure(err.into()))?;
                }
                Ok(())
            })?;
        }
        "profile" => {
            let db = positional(sub_matches, "db")?;
            let idx = Indexer::open(&db).map_err(|err| RunError::Failure(err.into()))?;
            let count = idx
                .all_blocks()
                .map_err(|err| RunError::Failure(err.into()))?
                .len();
            println!("indexed blocks: {count}");
        }
        "index-receipts" => {
            let dir = positional(sub_matches, "dir")?;
            let db = positional(sub_matches, "db")?;
            let idx = Indexer::open(&db).map_err(|err| RunError::Failure(err.into()))?;
            idx.index_receipts_dir(Path::new(&dir))
                .map_err(|err| RunError::Failure(err.into()))?;
        }
        other => return Err(RunError::Usage(format!("unknown subcommand '{other}'"))),
    }

    Ok(())
}

fn positional(matches: &Matches, name: &str) -> Result<String, RunError> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| RunError::Usage(format!("missing positional '{name}'")))
}

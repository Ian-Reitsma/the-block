use cli_core::{
    arg::{ArgSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use crypto_suite::hashing::blake3;
use foundation_archive::{gzip, tar};
use std::fs::{self, File};
use std::path::PathBuf;

enum RunError {
    Usage(String),
    Failure(diagnostics::anyhow::Error),
}

fn package(os: String, out: PathBuf) -> std::io::Result<()> {
    let file = File::create(&out)?;
    let encoder = gzip::Encoder::new(file)?;
    let mut builder = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    let body = format!("Installer for {os}\n");
    builder.append_data(&mut header, "README.txt", body.as_bytes())?;
    let encoder = builder.finish()?;
    let file = encoder.finish()?;
    file.sync_all()?;
    drop(file);

    let bytes = fs::read(&out)?;
    let sig = blake3::hash(&bytes);
    fs::write(out.with_extension("sig"), sig.to_hex().to_string())?;
    Ok(())
}

fn update() -> diagnostics::anyhow::Result<()> {
    diagnostics::anyhow::bail!(
        "first-party self-update tooling is not yet available; download releases manually"
    )
}

fn main() {
    if let Err(err) = run() {
        match err {
            RunError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
            RunError::Failure(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }
}

fn run() -> Result<(), RunError> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "installer".to_string());
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
    CommandBuilder::new(
        CommandId("installer"),
        "installer",
        "Package and update The-Block binaries",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("installer.package"),
            "package",
            "Package binaries for a target OS and sign the archive",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "os",
            "Target operating system identifier",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "out",
            "Destination archive path",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("installer.update"),
            "update",
            "Update the running binary from GitHub releases",
        )
        .build(),
    )
    .build()
}

fn handle_matches(matches: Matches) -> Result<(), RunError> {
    let (name, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| RunError::Usage("missing subcommand".into()))?;

    match name {
        "package" => {
            let os = positional(sub_matches, "os")?;
            let out = PathBuf::from(positional(sub_matches, "out")?);
            package(os, out).map_err(|err| RunError::Failure(err.into()))
        }
        "update" => update().map_err(|err| RunError::Failure(err.into())),
        other => Err(RunError::Usage(format!("unknown subcommand '{other}'"))),
    }
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

use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use foundation_serialization::json;
#[cfg(feature = "hid")]
use hidapi::HidApi;
use qrcode::{render::unicode, QrCode};
use std::fs;
use wallet::{psbt::Psbt, Wallet, WalletSigner};

enum RunError {
    Usage(String),
    Failure(String),
}

fn main() {
    if let Err(err) = run() {
        match err {
            RunError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
            RunError::Failure(msg) => {
                eprintln!("{msg}");
                std::process::exit(1);
            }
        }
    }
}

fn run() -> Result<(), RunError> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "remote-signer".to_string());
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

    let input = matches
        .get_string("input")
        .ok_or_else(|| RunError::Usage("missing required '--input' option".into()))?;
    let output = matches
        .get_string("output")
        .ok_or_else(|| RunError::Usage("missing required '--output' option".into()))?;
    let qr = matches.get_flag("qr");

    let data = fs::read(&input).map_err(|err| RunError::Failure(err.to_string()))?;
    let mut psbt: Psbt = json::from_slice(&data)
        .map_err(|err| RunError::Failure(format!("failed to parse PSBT: {err}")))?;
    let wallet = Wallet::generate();
    let sig = wallet
        .sign(&psbt.payload)
        .map_err(|err| RunError::Failure(format!("failed to sign PSBT payload: {err}")))?;
    psbt.add_signature(sig);
    let out = json::to_vec(&psbt)
        .map_err(|err| RunError::Failure(format!("failed to encode PSBT: {err}")))?;
    fs::write(&output, &out).map_err(|err| RunError::Failure(err.to_string()))?;
    if qr {
        if let Ok(code) = QrCode::new(&out) {
            let image = code.render::<unicode::Dense1x2>().build();
            println!("{}", image);
        }
    }
    Ok(())
}

fn build_command() -> Command {
    CommandBuilder::new(
        CommandId("remote-signer"),
        "remote-signer",
        "Simple remote signer CLI supporting air-gapped PSBT workflows",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("input", "input", "Input PSBT file path").required(true),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("output", "output", "Output PSBT file path").required(true),
    ))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "qr",
        "qr",
        "Render signed payload as QR to stdout",
    )))
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

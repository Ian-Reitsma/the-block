use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use http_env::blocking_client as env_blocking_client;
use httpd::{BlockingClient, Method};
use foundation_serialization::json;
use wallet::{Wallet, WalletSigner};

#[derive(Copy, Clone)]
enum Role {
    Gateway,
    Storage,
    Exec,
}

const TLS_PREFIXES: &[&str] = &["TB_RPC_TLS", "TB_HTTP_TLS"];

fn main() {
    if let Err(err) = run_cli() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run_cli() -> Result<(), String> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "wallet".to_string());
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

    match matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())? {
        ("stake-role", sub) => handle_stake_role(sub),
        ("escrow-balance", sub) => handle_escrow_balance(sub),
        (other, _) => Err(format!("unknown subcommand '{other}'")),
    }
}

fn handle_stake_role(matches: &Matches) -> Result<(), String> {
    let role_str = matches
        .get_positional("role")
        .and_then(|vals| vals.first().cloned())
        .ok_or_else(|| "missing role".to_string())?;
    let role = Role::from_str(&role_str)?;
    let amount = matches
        .get_positional("amount")
        .and_then(|vals| vals.first().cloned())
        .ok_or_else(|| "missing amount".to_string())?
        .parse::<u64>()
        .map_err(|err| err.to_string())?;
    let seed = matches
        .get_string("seed")
        .ok_or_else(|| "missing required '--seed' option".to_string())?;
    let withdraw = matches.get_flag("withdraw");
    let url = matches
        .get_string("url")
        .unwrap_or_else(|| "http://127.0.0.1:8545".to_string());

    let bytes = crypto_suite::hex::decode(seed).map_err(|err| err.to_string())?;
    if bytes.len() < 32 {
        return Err("seed too short".into());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes[..32]);
    let wallet = Wallet::from_seed(&arr);
    let role_label = role.as_str();
    let sig = wallet
        .sign_stake(role_label, amount, withdraw)
        .map_err(|err| err.to_string())?;
    let sig_hex = crypto_suite::hex::encode(sig.to_bytes());
    let pk_hex = wallet.public_key_hex();
    let body = foundation_serialization::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": if withdraw { "consensus.pos.unbond" } else { "consensus.pos.bond" },
        "params": {
            "id": pk_hex.clone(),
            "role": role_label,
            "amount": amount,
            "sig": sig_hex.clone(),
            "signers": [{"pk": pk_hex, "sig": sig_hex}],
            "threshold": 1,
        }
    });
    let client = env_blocking_client(TLS_PREFIXES, "examples::wallet");
    match client
        .request(Method::Post, &url)
        .and_then(|builder| builder.json(&body))
        .and_then(|builder| builder.send())
    {
        Ok(resp) => match resp.json::<json::Value>() {
            Ok(v) => println!("{}", v["result"].to_string()),
            Err(e) => return Err(format!("parse error: {e}")),
        },
        Err(e) => return Err(format!("rpc error: {e}")),
    }
    Ok(())
}

fn handle_escrow_balance(matches: &Matches) -> Result<(), String> {
    let account = matches
        .get_positional("account")
        .and_then(|vals| vals.first().cloned())
        .ok_or_else(|| "missing account".to_string())?;
    let url = matches
        .get_string("url")
        .unwrap_or_else(|| "http://127.0.0.1:8545".to_string());

    let payload = foundation_serialization::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "rent.escrow.balance",
        "params": {"id": account},
    });
    let client = env_blocking_client(TLS_PREFIXES, "examples::wallet");
    match client
        .request(Method::Post, &url)
        .and_then(|builder| builder.json(&payload))
        .and_then(|builder| builder.send())
    {
        Ok(resp) => match resp.json::<json::Value>() {
            Ok(v) => println!("{}", v["result"].as_u64().unwrap_or(0)),
            Err(e) => return Err(format!("parse error: {e}")),
        },
        Err(e) => return Err(format!("rpc error: {e}")),
    }
    Ok(())
}

fn build_command() -> Command {
    CommandBuilder::new(CommandId("wallet"), "wallet", "Wallet utilities")
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.stake-role"),
                "stake-role",
                "Stake CT for a service role",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "role",
                "Role to stake (gateway|storage|exec)",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Amount of CT to stake",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("seed", "seed", "32-byte seed in hex").required(true),
            ))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "withdraw",
                "withdraw",
                "Withdraw instead of bond",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint")
                    .default("http://127.0.0.1:8545"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.escrow-balance"),
                "escrow-balance",
                "Query rent-escrow balance for an account",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "account",
                "Account identifier",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "RPC endpoint")
                    .default("http://127.0.0.1:8545"),
            ))
            .build(),
        )
        .build()
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for details.");
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

impl Role {
    fn from_str(value: &str) -> Result<Self, String> {
        match value.to_lowercase().as_str() {
            "gateway" => Ok(Role::Gateway),
            "storage" => Ok(Role::Storage),
            "exec" => Ok(Role::Exec),
            other => Err(format!("unknown role '{other}'")),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Role::Gateway => "gateway",
            Role::Storage => "storage",
            Role::Exec => "exec",
        }
    }
}


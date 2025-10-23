#![deny(warnings)]

use std::{env, fs, path::PathBuf, process};

use ai::AiCmd;
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{Matches, ParseError, Parser},
};
use compute::ComputeCmd;
use config::ConfigCmd;
use dex::DexCmd;
use difficulty::DifficultyCmd;
use explorer::ExplorerCmd;
use gateway::GatewayCmd;
use inhouse::Dispatch as InhouseDispatch;
use parse_utils::{
    parse_positional_u64, parse_u64_required, parse_vec_u64, require_positional, require_string,
    take_string,
};
use system::SystemCmd;
use the_block::vm::{opcodes, ContractTx, Vm, VmType};
use tls::handle as handle_tls;
use tls::TlsCmd;
use version::VersionCmd;

use crate::fee_estimator::RollingMedianEstimator;

mod ai;
mod bridge;
mod codec_helpers;
mod compute;
mod config;
mod debug_cli;
mod dex;
mod difficulty;
mod explorer;
mod fee_estimator;
mod gateway;
mod gov;
mod htlc;
mod http_client;
mod identity;
mod inhouse;
mod json_helpers;
mod light_client;
mod light_sync;
mod logs;
mod net;
mod parse_utils;
mod remediation;
mod rpc;
mod scheduler;
mod service_badge;
mod snark;
mod storage;
mod system;
mod telemetry;
mod tls;
mod tx;
mod version;
#[cfg(feature = "quantum")]
mod wallet;
mod wasm;

use crate::wasm::extract_wasm_metadata;
use ai::handle as handle_ai;
use bridge::handle as handle_bridge;
use bridge::BridgeCmd;
use compute::handle as handle_compute;
use config::handle as handle_config;
use debug_cli::run as run_debugger;
use dex::handle as handle_dex;
use difficulty::handle as handle_difficulty;
use explorer::handle as handle_explorer;
use gateway::handle as handle_gateway;
use gov::handle as handle_gov;
use gov::GovCmd;
use htlc::handle as handle_htlc;
use htlc::HtlcCmd;
use identity::handle as handle_identity;
use identity::IdentityCmd;
use light_client::handle as handle_light_client;
use light_client::LightClientCmd;
use light_sync::handle as handle_light_sync;
use light_sync::LightSyncCmd;
use logs::handle as handle_logs;
use logs::LogCmd;
use net::handle as handle_net;
use net::NetCmd;
use remediation::handle as handle_remediation;
use remediation::RemediationCmd;
use scheduler::handle as handle_scheduler;
use scheduler::SchedulerCmd;
use service_badge::handle as handle_service_badge;
use service_badge::ServiceBadgeCmd;
use snark::handle as handle_snark;
use snark::SnarkCmd;
use storage::handle as handle_storage;
use storage::StorageCmd;
use system::handle as handle_system;
use telemetry::handle as handle_telemetry;
use telemetry::TelemetryCmd;
use version::handle as handle_version;
#[cfg(feature = "quantum")]
use wallet::handle as handle_wallet;
#[cfg(feature = "quantum")]
use wallet::WalletCmd;

fn main() {
    let mut argv = env::args();
    let bin = argv.next().unwrap_or_else(|| "contract".to_string());
    let args: Vec<String> = argv.collect();

    if args.is_empty() {
        print_root_help(&bin);
        return;
    }

    match inhouse::dispatch(&args) {
        InhouseDispatch::Handled | InhouseDispatch::HelpDisplayed => return,
        InhouseDispatch::Error(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
        InhouseDispatch::Unhandled => {}
    }

    let command = build_root_command();
    let parser = Parser::new(&command);

    match parser.parse(&args) {
        Ok(matches) => {
            if let Err(err) = handle_matches(matches) {
                eprintln!("{err}");
                process::exit(2);
            }
        }
        Err(ParseError::HelpRequested(path)) => {
            let tokens: Vec<&str> = path.split_whitespace().collect();
            print_help_for_path(&command, &tokens);
        }
        Err(ParseError::UnknownSubcommand(sub)) => {
            eprintln!("unknown subcommand '{sub}'");
            process::exit(2);
        }
        Err(ParseError::UnknownOption(option)) => {
            eprintln!("unknown option '--{option}'");
            process::exit(2);
        }
        Err(ParseError::MissingValue(option)) => {
            eprintln!("missing value for '--{option}'");
            process::exit(2);
        }
        Err(ParseError::MissingOption(name)) => {
            eprintln!("missing required option '--{name}'");
            process::exit(2);
        }
        Err(ParseError::MissingPositional(name)) => {
            eprintln!("missing positional argument '{name}'");
            process::exit(2);
        }
        Err(ParseError::InvalidChoice { option, value }) => {
            eprintln!("invalid value '{value}' for '--{option}'");
            process::exit(2);
        }
        Err(ParseError::InvalidValue {
            option,
            value,
            expected,
        }) => {
            eprintln!("invalid value '{value}' for '--{option}': expected {expected}");
            process::exit(2);
        }
    }
}
fn handle_matches(matches: Matches) -> Result<(), String> {
    let (name, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    match name {
        "deploy" => {
            handle_deploy(sub_matches)?;
            Ok(())
        }
        "call" => {
            handle_call(sub_matches)?;
            Ok(())
        }
        "abi" => {
            handle_abi(sub_matches)?;
            Ok(())
        }
        "debug" => {
            handle_debug(sub_matches)?;
            Ok(())
        }
        "fees" => {
            handle_fees(sub_matches)?;
            Ok(())
        }
        "config" => {
            let cmd = ConfigCmd::from_matches(sub_matches)?;
            handle_config(cmd);
            Ok(())
        }
        "version" => {
            let cmd = VersionCmd::from_matches(sub_matches)?;
            handle_version(cmd);
            Ok(())
        }
        "ai" => {
            let cmd = AiCmd::from_matches(sub_matches)?;
            handle_ai(cmd);
            Ok(())
        }
        "compute" => {
            let cmd = ComputeCmd::from_matches(sub_matches)?;
            handle_compute(cmd);
            Ok(())
        }
        "dex" => {
            let cmd = DexCmd::from_matches(sub_matches)?;
            handle_dex(cmd);
            Ok(())
        }
        "difficulty" => {
            let cmd = DifficultyCmd::from_matches(sub_matches)?;
            handle_difficulty(cmd);
            Ok(())
        }
        "explorer" => {
            let cmd = ExplorerCmd::from_matches(sub_matches)?;
            handle_explorer(cmd);
            Ok(())
        }
        "gateway" => {
            let cmd = GatewayCmd::from_matches(sub_matches)?;
            handle_gateway(cmd);
            Ok(())
        }
        "identity" => {
            let cmd = IdentityCmd::from_matches(sub_matches)?;
            handle_identity(cmd);
            Ok(())
        }
        "system" => {
            let cmd = SystemCmd::from_matches(sub_matches)?;
            handle_system(cmd);
            Ok(())
        }
        "tls" => {
            let cmd = TlsCmd::from_matches(sub_matches)?;
            handle_tls(cmd)?;
            Ok(())
        }
        "bridge" => {
            let cmd = BridgeCmd::from_matches(sub_matches)?;
            handle_bridge(cmd);
            Ok(())
        }
        "remediation" => {
            let cmd = RemediationCmd::from_matches(sub_matches)?;
            handle_remediation(cmd);
            Ok(())
        }
        "gov" => {
            let cmd = GovCmd::from_matches(sub_matches)?;
            handle_gov(cmd);
            Ok(())
        }
        "htlc" => {
            let cmd = HtlcCmd::from_matches(sub_matches)?;
            handle_htlc(cmd);
            Ok(())
        }
        "light-sync" => {
            let cmd = LightSyncCmd::from_matches(sub_matches)?;
            handle_light_sync(cmd);
            Ok(())
        }
        "light-client" => {
            let cmd = LightClientCmd::from_matches(sub_matches)?;
            handle_light_client(cmd);
            Ok(())
        }
        "logs" => {
            let cmd = LogCmd::from_matches(sub_matches)?;
            handle_logs(cmd);
            Ok(())
        }
        "net" => {
            let cmd = NetCmd::from_matches(sub_matches)?;
            handle_net(cmd);
            Ok(())
        }
        "scheduler" => {
            let cmd = SchedulerCmd::from_matches(sub_matches)?;
            handle_scheduler(cmd);
            Ok(())
        }
        "service-badge" => {
            let cmd = ServiceBadgeCmd::from_matches(sub_matches)?;
            handle_service_badge(cmd);
            Ok(())
        }
        "snark" => {
            let cmd = SnarkCmd::from_matches(sub_matches)?;
            handle_snark(cmd);
            Ok(())
        }
        "storage" => {
            let cmd = StorageCmd::from_matches(sub_matches)?;
            handle_storage(cmd);
            Ok(())
        }
        "telemetry" => {
            let cmd = TelemetryCmd::from_matches(sub_matches)?;
            handle_telemetry(cmd);
            Ok(())
        }
        #[cfg(feature = "quantum")]
        "wallet" => {
            let cmd = WalletCmd::from_matches(sub_matches)?;
            handle_wallet(cmd);
            Ok(())
        }
        "completions" => {
            handle_completions(sub_matches)?;
            Ok(())
        }
        other => Err(format!("unknown subcommand '{other}'")),
    }
}

fn build_root_command() -> Command {
    let builder = CommandBuilder::new(CommandId("contract"), "contract", "Contract management CLI")
        .subcommand(build_deploy_command())
        .subcommand(build_call_command())
        .subcommand(build_abi_command())
        .subcommand(build_debug_command())
        .subcommand(build_fees_command())
        .subcommand(ConfigCmd::command())
        .subcommand(VersionCmd::command())
        .subcommand(AiCmd::command())
        .subcommand(ComputeCmd::command())
        .subcommand(DexCmd::command())
        .subcommand(DifficultyCmd::command())
        .subcommand(ExplorerCmd::command())
        .subcommand(GatewayCmd::command())
        .subcommand(IdentityCmd::command())
        .subcommand(SystemCmd::command())
        .subcommand(TlsCmd::command())
        .subcommand(BridgeCmd::command())
        .subcommand(RemediationCmd::command())
        .subcommand(GovCmd::command())
        .subcommand(HtlcCmd::command())
        .subcommand(LightSyncCmd::command())
        .subcommand(LightClientCmd::command())
        .subcommand(LogCmd::command())
        .subcommand(NetCmd::command())
        .subcommand(SchedulerCmd::command())
        .subcommand(ServiceBadgeCmd::command())
        .subcommand(SnarkCmd::command())
        .subcommand(StorageCmd::command())
        .subcommand(TelemetryCmd::command());

    #[cfg(feature = "quantum")]
    let builder = builder.subcommand(WalletCmd::command());

    builder.subcommand(build_completions_command()).build()
}

fn build_deploy_command() -> Command {
    CommandBuilder::new(
        CommandId("contract.deploy"),
        "deploy",
        "Deploy contract code",
    )
    .arg(ArgSpec::Positional(
        PositionalSpec::new("code", "Hex-encoded contract code").optional(),
    ))
    .arg(ArgSpec::Option(OptionSpec::new(
        "wasm",
        "wasm",
        "WASM file containing the contract",
    )))
    .arg(ArgSpec::Option(
        OptionSpec::new("state", "state", "State snapshot path").default("contracts.bin"),
    ))
    .build()
}

fn build_call_command() -> Command {
    CommandBuilder::new(
        CommandId("contract.call"),
        "call",
        "Call a deployed contract",
    )
    .arg(ArgSpec::Positional(PositionalSpec::new(
        "id",
        "Contract identifier",
    )))
    .arg(ArgSpec::Positional(PositionalSpec::new(
        "input",
        "Hex-encoded input",
    )))
    .arg(ArgSpec::Option(
        OptionSpec::new("state", "state", "State snapshot path").default("contracts.bin"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("gas_limit", "gas-limit", "Gas limit").default("50"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("gas_price", "gas-price", "Gas price").default("1"),
    ))
    .build()
}

fn build_abi_command() -> Command {
    CommandBuilder::new(CommandId("contract.abi"), "abi", "Generate opcode ABI JSON")
        .arg(ArgSpec::Positional(
            PositionalSpec::new("out", "Output file path").optional(),
        ))
        .build()
}

fn build_debug_command() -> Command {
    CommandBuilder::new(
        CommandId("contract.debug"),
        "debug",
        "Start the interactive VM debugger",
    )
    .arg(ArgSpec::Positional(PositionalSpec::new(
        "code",
        "Hex-encoded contract code",
    )))
    .build()
}

fn build_fees_command() -> Command {
    CommandBuilder::new(
        CommandId("contract.fees"),
        "fees",
        "Estimate fees from observed samples",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("samples", "samples", "Comma separated tip samples")
            .value_delimiter(',')
            .multiple(true),
    ))
    .build()
}

fn build_completions_command() -> Command {
    CommandBuilder::new(
        CommandId("contract.completions"),
        "completions",
        "Generate shell completion scripts",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("shell", "shell", "Shell to target")
            .required(true)
            .value_enum(&["bash", "zsh", "fish"]),
    ))
    .build()
}

fn handle_deploy(matches: &Matches) -> Result<(), String> {
    let code = matches
        .get_positional("code")
        .and_then(|values| values.first().cloned());
    let wasm_path = take_string(matches, "wasm").map(PathBuf::from);
    let state = take_string(matches, "state").unwrap_or_else(|| "contracts.bin".to_string());

    let path = PathBuf::from(state);
    let mut vm = Vm::new_persistent(VmType::Wasm, path);
    if let Some(wasm) = wasm_path {
        let bytes = fs::read(&wasm).map_err(|err| format!("failed to read {:?}: {err}", wasm))?;
        let meta = extract_wasm_metadata(&bytes);
        let id = vm.deploy_wasm(bytes, meta);
        println!("{id}");
        return Ok(());
    }

    if let Some(code_hex) = code {
        let bytes = crypto_suite::hex::decode(&code_hex)
            .map_err(|_| "invalid hex code provided to deploy".to_string())?;
        let id = vm.deploy(bytes);
        println!("{id}");
        return Ok(());
    }

    Err("either positional <code> or --wasm must be provided".to_string())
}

fn handle_call(matches: &Matches) -> Result<(), String> {
    let id = parse_positional_u64(matches, "id")?;
    let input = require_positional(matches, "input")?;
    let state = take_string(matches, "state").unwrap_or_else(|| "contracts.bin".to_string());
    let gas_limit = parse_u64_required(take_string(matches, "gas_limit"), "gas-limit")?;
    let gas_price = parse_u64_required(take_string(matches, "gas_price"), "gas-price")?;

    let path = PathBuf::from(state);
    let mut vm = Vm::new_persistent(VmType::Wasm, path);
    let mut balance = u64::MAX;
    let bytes = crypto_suite::hex::decode(input).map_err(|_| "invalid hex input".to_string())?;
    let tx = ContractTx::Call {
        id,
        input: bytes,
        gas_limit,
        gas_price,
    };
    match tx.apply(&mut vm, &mut balance) {
        Ok(out) => {
            println!("{}", crypto_suite::hex::encode(out));
            Ok(())
        }
        Err(err) => {
            eprintln!("{err}");
            Err("contract execution failed".to_string())
        }
    }
}

fn handle_abi(matches: &Matches) -> Result<(), String> {
    let out = matches
        .get_positional("out")
        .and_then(|values| values.first().cloned())
        .unwrap_or_else(|| "opcodes.json".to_string());
    let path = PathBuf::from(out);
    opcodes::write_abi(&path).map_err(|err| format!("failed to write ABI: {err}"))
}

fn handle_debug(matches: &Matches) -> Result<(), String> {
    let code = require_positional(matches, "code")?;
    run_debugger(code);
    Ok(())
}

fn handle_fees(matches: &Matches) -> Result<(), String> {
    let raw_samples = matches.get_strings("samples");
    let samples = parse_vec_u64(raw_samples, "samples")?;
    let mut estimator = RollingMedianEstimator::new(21);
    for sample in samples {
        estimator.record(sample);
    }
    println!("{}", estimator.suggest());
    Ok(())
}

fn handle_completions(matches: &Matches) -> Result<(), String> {
    let shell = require_string(matches, "shell")?;
    match shell.as_str() {
        "bash" => {
            println!("{}_completion() {{", "contract");
            println!("    local cur prev");
            println!("    COMPREPLY=()");
            println!("    cur=\"${{COMP_WORDS[COMP_CWORD]}}\"");
            println!("    prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"");
            println!("    if [[ $COMP_CWORD -eq 1 ]]; then");
            println!(
                "        COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                all_command_names().join(" ")
            );
            println!("    fi");
            println!("    return 0");
            println!("}}");
            println!("complete -F {}_completion contract", "contract");
            Ok(())
        }
        "zsh" => {
            println!("#compdef contract");
            println!("_arguments '1: :({})'", all_command_names().join(" "));
            Ok(())
        }
        "fish" => {
            for name in all_command_names() {
                println!(
                    "complete -c contract -f -n '__fish_use_subcommand' -a {}",
                    name
                );
            }
            Ok(())
        }
        other => Err(format!("unsupported shell '{other}'")),
    }
}

fn print_root_help(bin: &str) {
    let command = build_root_command();
    let generator = HelpGenerator::new(&command);
    println!("{}", generator.render());
    println!("\nRun '{bin} <subcommand> --help' for details on a command.");
}

fn print_help_for_path(root: &Command, path: &[&str]) {
    if let Some(cmd) = find_command(root, path) {
        let generator = HelpGenerator::new(cmd);
        println!("{}", generator.render());
    }
}

fn find_command<'a>(command: &'a Command, path: &[&str]) -> Option<&'a Command> {
    if path.is_empty() {
        return Some(command);
    }

    let mut current = command;
    for segment in path.iter().skip(1) {
        if let Some(next) = current.subcommands.iter().find(|cmd| cmd.name == *segment) {
            current = next;
        } else {
            return None;
        }
    }
    Some(current)
}

fn all_command_names() -> Vec<&'static str> {
    vec![
        "deploy",
        "call",
        "abi",
        "bridge",
        "dex",
        "compute",
        "net",
        "gateway",
        "logs",
        "difficulty",
        "gov",
        "explorer",
        "version",
        "config",
        "telemetry",
        "debug",
        "service-badge",
        "htlc",
        "storage",
        "scheduler",
        "snark",
        "light-sync",
        "light-client",
        "system",
        "ai",
        "fees",
        "completions",
        #[cfg(feature = "quantum")]
        "wallet",
    ]
}

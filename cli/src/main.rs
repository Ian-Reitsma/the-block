#![deny(warnings)]

use std::{env, fs, path::PathBuf, process};

use ai::AiCmd;
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
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
    parse_positional_u64, parse_u64, parse_u64_required, parse_usize, parse_vec_u64,
    require_positional, require_string, take_string,
};
use system::SystemCmd;
use the_block::vm::{opcodes, ContractTx, Vm, VmType};
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
mod inhouse;
mod light_client;
mod light_sync;
mod logs;
mod net;
mod parse_utils;
mod rpc;
mod scheduler;
mod service_badge;
mod snark;
mod storage;
mod system;
mod telemetry;
mod tx;
mod version;
#[cfg(feature = "quantum")]
mod wallet;

use ai::handle as handle_ai;
use compute::handle as handle_compute;
use config::handle as handle_config;
use debug_cli::run as run_debugger;
use dex::handle as handle_dex;
use difficulty::handle as handle_difficulty;
use explorer::handle as handle_explorer;
use gateway::handle as handle_gateway;
use logs::LogCmd;
use net::NetCmd;
use scheduler::SchedulerCmd;
use service_badge::ServiceBadgeCmd;
use snark::SnarkCmd;
use storage::StorageCmd;
use system::handle as handle_system;
use telemetry::TelemetryCmd;
use version::handle as handle_version;
#[cfg(feature = "quantum")]
use wallet::WalletCmd;

const PORTED_COMMANDS: &[&str] = &[
    "deploy",
    "call",
    "abi",
    "debug",
    "fees",
    "config",
    "version",
    "ai",
    "compute",
    "dex",
    "difficulty",
    "explorer",
    "gateway",
    "system",
    "completions",
];

#[cfg(feature = "wasm-metadata")]
fn extract_wasm_metadata(bytes: &[u8]) -> Vec<u8> {
    let engine = wasmtime::Engine::default();
    if let Ok(module) = wasmtime::Module::new(&engine, bytes) {
        let exports: Vec<String> = module.exports().map(|e| e.name().to_string()).collect();
        codec_helpers::json_to_vec(&exports).unwrap_or_default()
    } else {
        Vec::new()
    }
}

#[cfg(not(feature = "wasm-metadata"))]
fn extract_wasm_metadata(_bytes: &[u8]) -> Vec<u8> {
    Vec::new()
}

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

    match dispatch_cli_core(&bin, &args) {
        CliCoreOutcome::Handled => {}
        CliCoreOutcome::HelpPrinted => {}
        CliCoreOutcome::Error(code) => process::exit(code),
        CliCoreOutcome::Fallback => {
            let mut legacy_args = Vec::with_capacity(args.len() + 1);
            legacy_args.push(bin.clone());
            legacy_args.extend(args.iter().cloned());
            let status = legacy::run(&legacy_args);
            process::exit(status);
        }
    }
}

enum CliCoreOutcome {
    Handled,
    HelpPrinted,
    Fallback,
    Error(i32),
}

fn dispatch_cli_core(bin: &str, args: &[String]) -> CliCoreOutcome {
    let command = build_root_command();
    let parser = Parser::new(&command);

    match parser.parse(args) {
        Ok(matches) => match handle_matches(matches) {
            Ok(true) => CliCoreOutcome::Handled,
            Ok(false) => CliCoreOutcome::Fallback,
            Err(err) => {
                eprintln!("{err}");
                CliCoreOutcome::Error(2)
            }
        },
        Err(ParseError::HelpRequested(path)) => {
            let tokens: Vec<&str> = path.split_whitespace().collect();
            print_help_for_path(&command, &tokens);
            CliCoreOutcome::HelpPrinted
        }
        Err(ParseError::UnknownSubcommand(sub)) => {
            if is_ported(args) {
                eprintln!("unknown subcommand '{sub}'");
                CliCoreOutcome::Error(2)
            } else {
                CliCoreOutcome::Fallback
            }
        }
        Err(ParseError::UnknownOption(option)) => {
            if is_ported(args) {
                eprintln!("unknown option '--{option}'");
                CliCoreOutcome::Error(2)
            } else {
                CliCoreOutcome::Fallback
            }
        }
        Err(ParseError::MissingValue(option)) => {
            if is_ported(args) {
                eprintln!("missing value for '--{option}'");
                CliCoreOutcome::Error(2)
            } else {
                CliCoreOutcome::Fallback
            }
        }
        Err(ParseError::MissingOption(name)) => {
            eprintln!("missing required option '--{name}'");
            CliCoreOutcome::Error(2)
        }
        Err(ParseError::MissingPositional(name)) => {
            eprintln!("missing positional argument '{name}'");
            CliCoreOutcome::Error(2)
        }
        Err(ParseError::InvalidChoice { option, value }) => {
            eprintln!("invalid value '{value}' for '--{option}'");
            CliCoreOutcome::Error(2)
        }
        Err(ParseError::InvalidValue {
            option,
            value,
            expected,
        }) => {
            eprintln!("invalid value '{value}' for '--{option}': expected {expected}");
            CliCoreOutcome::Error(2)
        }
    }
}

fn is_ported(args: &[String]) -> bool {
    args.first()
        .map(|first| PORTED_COMMANDS.contains(&first.as_str()))
        .unwrap_or(false)
}

fn handle_matches(matches: Matches) -> Result<bool, String> {
    let (name, sub_matches) = match matches.subcommand() {
        Some(pair) => pair,
        None => return Ok(false),
    };

    match name {
        "deploy" => {
            handle_deploy(sub_matches)?;
            Ok(true)
        }
        "call" => {
            handle_call(sub_matches)?;
            Ok(true)
        }
        "abi" => {
            handle_abi(sub_matches)?;
            Ok(true)
        }
        "debug" => {
            handle_debug(sub_matches)?;
            Ok(true)
        }
        "fees" => {
            handle_fees(sub_matches)?;
            Ok(true)
        }
        "config" => {
            let cmd = ConfigCmd::from_matches(sub_matches)?;
            handle_config(cmd);
            Ok(true)
        }
        "version" => {
            let cmd = VersionCmd::from_matches(sub_matches)?;
            handle_version(cmd);
            Ok(true)
        }
        "ai" => {
            let cmd = AiCmd::from_matches(sub_matches)?;
            handle_ai(cmd);
            Ok(true)
        }
        "compute" => {
            let cmd = ComputeCmd::from_matches(sub_matches)?;
            handle_compute(cmd);
            Ok(true)
        }
        "dex" => {
            let cmd = DexCmd::from_matches(sub_matches)?;
            handle_dex(cmd);
            Ok(true)
        }
        "difficulty" => {
            let cmd = DifficultyCmd::from_matches(sub_matches)?;
            handle_difficulty(cmd);
            Ok(true)
        }
        "explorer" => {
            let cmd = ExplorerCmd::from_matches(sub_matches)?;
            handle_explorer(cmd);
            Ok(true)
        }
        "gateway" => {
            let cmd = GatewayCmd::from_matches(sub_matches)?;
            handle_gateway(cmd);
            Ok(true)
        }
        "system" => {
            let cmd = SystemCmd::from_matches(sub_matches)?;
            handle_system(cmd);
            Ok(true)
        }
        "completions" => {
            handle_completions(sub_matches)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn build_root_command() -> Command {
    CommandBuilder::new(CommandId("contract"), "contract", "Contract management CLI")
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
        .subcommand(SystemCmd::command())
        .subcommand(build_completions_command())
        .allow_external_subcommands(true)
        .build()
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
        let bytes = hex::decode(&code_hex)
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
    let bytes = hex::decode(input).map_err(|_| "invalid hex input".to_string())?;
    let tx = ContractTx::Call {
        id,
        input: bytes,
        gas_limit,
        gas_price,
    };
    match tx.apply(&mut vm, &mut balance) {
        Ok(out) => {
            println!("{}", hex::encode(out));
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

mod legacy {
    use super::*;
    #[cfg(feature = "quantum")]
    use crate::wallet;
    use crate::{
        bridge, gov, htlc, light_client, light_sync, logs, net, scheduler, service_badge, snark,
        storage, telemetry,
    };
    use clap::{CommandFactory, Parser, Subcommand};

    #[derive(Parser)]
    #[command(name = "contract")]
    struct LegacyCli {
        #[command(subcommand)]
        cmd: LegacyCommand,
    }

    #[derive(Subcommand)]
    enum LegacyCommand {
        /// Bridge deposit and withdraw
        Bridge {
            #[command(subcommand)]
            action: bridge::BridgeCmd,
        },
        /// Networking utilities
        Net {
            #[command(subcommand)]
            action: NetCmd,
        },
        /// Governance utilities
        Gov {
            #[command(subcommand)]
            action: gov::GovCmd,
        },
        /// Light client synchronization
        LightSync {
            #[command(subcommand)]
            action: light_sync::LightSyncCmd,
        },
        /// Light client utilities
        LightClient {
            #[command(subcommand)]
            action: light_client::LightClientCmd,
        },
        /// Service badge utilities
        ServiceBadge {
            #[command(subcommand)]
            action: ServiceBadgeCmd,
        },
        /// HTLC utilities
        Htlc {
            #[command(subcommand)]
            action: htlc::HtlcCmd,
        },
        /// Storage market utilities
        Storage {
            #[command(subcommand)]
            action: StorageCmd,
        },
        /// Scheduler diagnostics
        Scheduler {
            #[command(subcommand)]
            action: SchedulerCmd,
        },
        /// SNARK tooling
        Snark {
            #[command(subcommand)]
            action: SnarkCmd,
        },
        /// Log search utilities
        Logs {
            #[command(subcommand)]
            action: LogCmd,
        },
        /// Telemetry diagnostics
        Telemetry {
            #[command(subcommand)]
            action: TelemetryCmd,
        },
        /// Wallet utilities
        #[cfg(feature = "quantum")]
        Wallet {
            #[command(subcommand)]
            action: WalletCmd,
        },
    }

    pub fn run(args: &[String]) -> i32 {
        match LegacyCli::try_parse_from(args) {
            Ok(cli) => {
                match cli.cmd {
                    LegacyCommand::Bridge { action } => bridge::handle(action),
                    LegacyCommand::Net { action } => net::handle(action),
                    LegacyCommand::Gov { action } => gov::handle(action),
                    LegacyCommand::LightSync { action } => light_sync::handle(action),
                    LegacyCommand::LightClient { action } => light_client::handle(action),
                    LegacyCommand::ServiceBadge { action } => service_badge::handle(action),
                    LegacyCommand::Htlc { action } => htlc::handle(action),
                    LegacyCommand::Storage { action } => storage::handle(action),
                    LegacyCommand::Scheduler { action } => scheduler::handle(action),
                    LegacyCommand::Snark { action } => snark::handle(action),
                    LegacyCommand::Logs { action } => logs::handle(action),
                    LegacyCommand::Telemetry { action } => telemetry::handle(action),
                    #[cfg(feature = "quantum")]
                    LegacyCommand::Wallet { action } => wallet::handle(action),
                }
                0
            }
            Err(err) => {
                err.print().expect("failed to print Clap error");
                2
            }
        }
    }
}

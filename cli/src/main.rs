#![deny(warnings)]

use clap::{Parser, Subcommand};
use std::{fs, path::PathBuf};

mod ai;
mod bridge;
mod compute;
mod config;
mod debug_cli;
mod dex;
mod difficulty;
mod fee_estimator;
mod gateway;
mod gov;
mod htlc;
mod light_client;
mod light_sync;
mod net;
mod logs;
mod service_badge;
mod scheduler;
mod snark;
mod storage;
mod telemetry;
mod version;
#[cfg(feature = "quantum")]
mod wallet;
use ai::AiCmd;
use bridge::BridgeCmd;
use compute::ComputeCmd;
use config::ConfigCmd;
use dex::DexCmd;
use difficulty::DifficultyCmd;
use gateway::GatewayCmd;
use gov::GovCmd;
use htlc::HtlcCmd;
use light_client::LightClientCmd;
use light_sync::LightSyncCmd;
use net::NetCmd;
use logs::LogCmd;
use service_badge::ServiceBadgeCmd;
use scheduler::SchedulerCmd;
use snark::SnarkCmd;
use storage::StorageCmd;
use telemetry::TelemetryCmd;
use the_block::vm::{opcodes, ContractTx, Vm, VmType};
use version::VersionCmd;
#[cfg(feature = "quantum")]
use wallet::WalletCmd;

fn extract_wasm_metadata(bytes: &[u8]) -> Vec<u8> {
    let engine = wasmtime::Engine::default();
    if let Ok(module) = wasmtime::Module::new(&engine, bytes) {
        let exports: Vec<String> = module.exports().map(|e| e.name().to_string()).collect();
        serde_json::to_vec(&exports).unwrap_or_default()
    } else {
        Vec::new()
    }
}

#[derive(Parser)]
#[command(name = "contract")]
#[command(about = "Contract management CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy contract code. Provide either hex `code` or a WASM file.
    Deploy {
        #[arg(required_unless_present = "wasm")]
        code: Option<String>,
        #[arg(long)]
        wasm: Option<PathBuf>,
        #[arg(long, default_value = "contracts.bin")]
        state: String,
    },
    /// Call a deployed contract
    Call {
        id: u64,
        input: String,
        #[arg(long, default_value = "contracts.bin")]
        state: String,
        #[arg(long, default_value_t = 50)]
        gas_limit: u64,
        #[arg(long, default_value_t = 1)]
        gas_price: u64,
    },
    /// Generate opcode ABI JSON
    Abi {
        #[arg(default_value = "opcodes.json")]
        out: String,
    },
    /// Bridge deposit and withdraw
    Bridge {
        #[command(subcommand)]
        action: BridgeCmd,
    },
    /// DEX escrow utilities
    Dex {
        #[command(subcommand)]
        action: DexCmd,
    },
    /// Compute marketplace utilities
    Compute {
        #[command(subcommand)]
        action: ComputeCmd,
    },
    /// Networking utilities
    Net {
        #[command(subcommand)]
        action: NetCmd,
    },
    /// Gateway operations
    Gateway {
        #[command(subcommand)]
        action: GatewayCmd,
    },
    /// Log search utilities
    Logs {
        #[command(subcommand)]
        action: LogCmd,
    },
    /// Difficulty utilities
    Difficulty {
        #[command(subcommand)]
        action: DifficultyCmd,
    },
    /// Governance utilities
    Gov {
        #[command(subcommand)]
        action: GovCmd,
    },
    /// Version and build info
    Version {
        #[command(subcommand)]
        action: VersionCmd,
    },
    /// Config utilities
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Telemetry diagnostics
    Telemetry {
        #[command(subcommand)]
        action: TelemetryCmd,
    },
    /// Interactive VM debugger
    Debug { code: String },
    /// Service badge utilities
    ServiceBadge {
        #[command(subcommand)]
        action: ServiceBadgeCmd,
    },
    /// HTLC utilities
    Htlc {
        #[command(subcommand)]
        action: HtlcCmd,
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
    /// Light client synchronization
    LightSync {
        #[command(subcommand)]
        action: LightSyncCmd,
    },
    /// Light client utilities
    LightClient {
        #[command(subcommand)]
        action: LightClientCmd,
    },
    /// AI diagnostics
    Ai {
        #[command(subcommand)]
        action: AiCmd,
    },
    /// Fee estimation utilities
    Fees {
        /// Recent observed tip samples
        #[arg(long, value_delimiter = ',')]
        samples: Vec<u64>,
    },
    #[cfg(feature = "quantum")]
    Wallet {
        #[command(subcommand)]
        action: WalletCmd,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Deploy { code, wasm, state } => {
            let path = PathBuf::from(state);
            let mut vm = Vm::new_persistent(VmType::Wasm, path);
            if let Some(w) = wasm {
                let bytes = fs::read(&w).expect("read wasm");
                let meta = extract_wasm_metadata(&bytes);
                let id = vm.deploy_wasm(bytes, meta);
                println!("{}", id);
            } else if let Some(code_hex) = code {
                let bytes = hex::decode(code_hex).expect("invalid hex code");
                let id = vm.deploy(bytes);
                println!("{}", id);
            } else {
                eprintln!("no code provided");
            }
        }
        Commands::Call {
            id,
            input,
            state,
            gas_limit,
            gas_price,
        } => {
            let path = PathBuf::from(state);
            let mut vm = Vm::new_persistent(VmType::Wasm, path);
            let mut bal = u64::MAX; // user pays separately in real chain
            let bytes = hex::decode(input).expect("invalid hex input");
            let tx = ContractTx::Call {
                id,
                input: bytes,
                gas_limit,
                gas_price,
            };
            match tx.apply(&mut vm, &mut bal) {
                Ok(out) => println!("{}", hex::encode(out)),
                Err(e) => eprintln!("{}", e),
            }
        }
        Commands::Abi { out } => {
            let path = PathBuf::from(out);
            opcodes::write_abi(&path).expect("write abi");
        }
        Commands::Bridge { action } => bridge::handle(action),
        Commands::Dex { action } => dex::handle(action),
        Commands::Compute { action } => compute::handle(action),
        Commands::Net { action } => net::handle(action),
        Commands::Gateway { action } => gateway::handle(action),
        Commands::Logs { action } => logs::handle(action),
        Commands::Difficulty { action } => difficulty::handle(action),
        Commands::Gov { action } => gov::handle(action),
        Commands::Config { action } => config::handle(action),
        Commands::Version { action } => version::handle(action),
        Commands::Telemetry { action } => telemetry::handle(action),
        Commands::Debug { code } => debug_cli::run(code),
        Commands::ServiceBadge { action } => service_badge::handle(action),
        Commands::Htlc { action } => htlc::handle(action),
        Commands::Storage { action } => storage::handle(action),
        Commands::Scheduler { action } => scheduler::handle(action),
        Commands::Snark { action } => snark::handle(action),
        Commands::LightSync { action } => light_sync::handle(action),
        Commands::LightClient { action } => light_client::handle(action),
        Commands::Ai { action } => ai::handle(action),
        Commands::Fees { samples } => {
            let mut est = fee_estimator::RollingMedianEstimator::new(21);
            for s in samples {
                est.record(s);
            }
            println!("{}", est.suggest());
        }
        #[cfg(feature = "quantum")]
        Commands::Wallet { action } => wallet::handle(action),
    }
}

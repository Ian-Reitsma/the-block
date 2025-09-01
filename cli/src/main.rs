use bridges::{Bridge, RelayerProof};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use the_block::vm::{opcodes, ContractTx, Vm, VmType};

#[derive(Parser)]
#[command(name = "contract")]
#[command(about = "Contract management CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy raw bytecode provided as hex string
    Deploy {
        code: String,
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
}

#[derive(Subcommand)]
enum BridgeCmd {
    Deposit {
        user: String,
        amount: u64,
        relayer: String,
        #[arg(long, default_value = "bridge.bin")]
        state: String,
    },
    Withdraw {
        user: String,
        amount: u64,
        relayer: String,
        #[arg(long, default_value = "bridge.bin")]
        state: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Deploy { code, state } => {
            let path = PathBuf::from(state);
            let mut vm = Vm::new_persistent(VmType::Wasm, path);
            let bytes = hex::decode(code).expect("invalid hex code");
            let id = vm.deploy(bytes);
            println!("{}", id);
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
        Commands::Bridge { action } => match action {
            BridgeCmd::Deposit {
                user,
                amount,
                relayer,
                state,
            } => {
                let path = PathBuf::from(state);
                let mut bridge = if path.exists() {
                    let bytes = fs::read(&path).expect("read bridge state");
                    bincode::deserialize(&bytes).unwrap_or_default()
                } else {
                    Bridge::default()
                };
                let proof = RelayerProof::new(&relayer, &user, amount);
                if bridge.lock(&user, amount, &proof) {
                    let bytes = bincode::serialize(&bridge).expect("serialize");
                    fs::write(&path, bytes).expect("write bridge state");
                    println!("locked");
                } else {
                    eprintln!("invalid proof");
                }
            }
            BridgeCmd::Withdraw {
                user,
                amount,
                relayer,
                state,
            } => {
                let path = PathBuf::from(state);
                let mut bridge = if path.exists() {
                    let bytes = fs::read(&path).expect("read bridge state");
                    bincode::deserialize(&bytes).unwrap_or_default()
                } else {
                    Bridge::default()
                };
                let proof = RelayerProof::new(&relayer, &user, amount);
                if bridge.unlock(&user, amount, &proof) {
                    let bytes = bincode::serialize(&bridge).expect("serialize");
                    fs::write(&path, bytes).expect("write bridge state");
                    println!("unlocked");
                } else {
                    eprintln!("invalid proof or balance");
                }
            }
        },
    }
}

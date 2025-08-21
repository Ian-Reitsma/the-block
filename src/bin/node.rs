use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;

#[cfg(feature = "telemetry")]
use the_block::serve_metrics;
use the_block::{
    compute_market::courier::CourierStore, generate_keypair, rpc::run_rpc_server, sign_tx,
    spawn_purge_loop_thread, Blockchain, RawTxPayload, ShutdownFlag,
};

fn key_dir() -> PathBuf {
    dirs::home_dir()
        .expect("home directory")
        .join(".the_block")
        .join("keys")
}

fn key_path(id: &str) -> PathBuf {
    key_dir().join(format!("{id}.pem"))
}

fn write_pem(path: &Path, sk: &SigningKey) -> std::io::Result<()> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    let pem = format!(
        "-----BEGIN ED25519 PRIVATE KEY-----\n{}\n-----END ED25519 PRIVATE KEY-----\n",
        B64.encode(sk.to_bytes())
    );
    fs::write(path, pem)
}

fn read_pem(src: &str) -> std::io::Result<SigningKey> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    let b64: String = src.lines().filter(|l| !l.starts_with("---")).collect();
    let bytes = B64
        .decode(b64)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "key length"))?;
    Ok(SigningKey::from_bytes(&arr))
}

fn load_key(id: &str) -> SigningKey {
    let path = key_path(id);
    let data = fs::read_to_string(path).expect("read key");
    read_pem(&data).expect("parse key")
}

#[derive(Parser)]
#[command(author, version, about = "Run a basic node or manage wallet keys")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a full node with JSON-RPC controls
    Run {
        /// Address to bind the JSON-RPC server to
        #[arg(long, default_value = "127.0.0.1:3030")]
        rpc_addr: String,

        /// Seconds between mempool purge sweeps (0 to disable)
        #[arg(long, default_value_t = 0)]
        mempool_purge_interval: u64,

        /// Interval in blocks between full snapshots
        #[arg(long, default_value_t = 600)]
        snapshot_interval: u64,

        /// Expose Prometheus metrics on this address (requires `--features telemetry`)
        #[arg(long, value_name = "ADDR")]
        metrics_addr: Option<String>,

        /// Directory for chain data
        #[arg(long, default_value = "node-data")]
        data_dir: String,
    },
    /// Generate a new keypair saved under ~/.the_block/keys/<key_id>.pem
    GenerateKey { key_id: String },
    /// Import an existing PEM-encoded key file
    ImportKey { file: String },
    /// Show the hex address for the given key id
    ShowAddress { key_id: String },
    /// Sign a transaction JSON payload with the given key
    SignTx { key_id: String, tx_json: String },
    /// Compute-related utilities
    Compute {
        #[command(subcommand)]
        cmd: ComputeCmd,
    },
}

#[derive(Subcommand)]
enum ComputeCmd {
    /// Courier receipt operations
    Courier {
        #[command(subcommand)]
        action: CourierCmd,
    },
}

#[derive(Subcommand)]
enum CourierCmd {
    /// Send a bundle and store a courier receipt
    Send { file: String, sender: String },
    /// Flush stored receipts
    Flush,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Run {
            rpc_addr,
            mempool_purge_interval,
            snapshot_interval,
            metrics_addr,
            data_dir,
        } => {
            let mut inner = Blockchain::new(&data_dir);
            inner.snapshot.set_interval(snapshot_interval);
            let bc = Arc::new(Mutex::new(inner));

            #[cfg(feature = "telemetry")]
            if let Some(addr) = &metrics_addr {
                let _ = serve_metrics(addr);
            }
            #[cfg(not(feature = "telemetry"))]
            if metrics_addr.is_some() {
                eprintln!("telemetry feature not enabled");
                return std::process::ExitCode::FAILURE;
            }

            if mempool_purge_interval > 0 {
                let flag = ShutdownFlag::new();
                spawn_purge_loop_thread(Arc::clone(&bc), mempool_purge_interval, flag.as_arc());
            }

            let mining = Arc::new(AtomicBool::new(false));
            let (tx, rx) = tokio::sync::oneshot::channel();
            let handle = tokio::spawn(run_rpc_server(
                Arc::clone(&bc),
                Arc::clone(&mining),
                rpc_addr.clone(),
                tx,
            ));
            let rpc_addr = rx.await.expect("rpc addr");
            println!("RPC listening on {rpc_addr}");
            let _ = handle.await;
            std::process::ExitCode::SUCCESS
        }
        Commands::GenerateKey { key_id } => {
            let (sk_bytes, _pk) = generate_keypair();
            let sk = SigningKey::from_bytes(&sk_bytes.try_into().expect("sk bytes"));
            fs::create_dir_all(key_dir()).expect("key dir");
            write_pem(&key_path(&key_id), &sk).expect("write key");
            println!("{}", hex::encode(sk.verifying_key().to_bytes()));
            std::process::ExitCode::SUCCESS
        }
        Commands::ImportKey { file } => {
            let data = match fs::read_to_string(&file) {
                Ok(d) => d,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    eprintln!("key file not found: {file}");
                    return std::process::ExitCode::FAILURE;
                }
                Err(e) => panic!("read key file: {e}"),
            };
            let sk = read_pem(&data).expect("parse key");
            fs::create_dir_all(key_dir()).expect("key dir");
            let key_id = Path::new(&file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("imported");
            fs::write(key_path(key_id), data).expect("write key");
            println!("{}", hex::encode(sk.verifying_key().to_bytes()));
            std::process::ExitCode::SUCCESS
        }
        Commands::ShowAddress { key_id } => {
            let sk = load_key(&key_id);
            println!("{}", hex::encode(sk.verifying_key().to_bytes()));
            std::process::ExitCode::SUCCESS
        }
        Commands::SignTx { key_id, tx_json } => {
            let sk = load_key(&key_id);
            let payload: RawTxPayload = serde_json::from_str(&tx_json).expect("parse tx payload");
            let signed = sign_tx(sk.to_bytes().to_vec(), payload).expect("sign tx");
            let bytes = bincode::serialize(&signed).expect("serialize tx");
            println!("{}", hex::encode(bytes));
            std::process::ExitCode::SUCCESS
        }
        Commands::Compute { cmd } => match cmd {
            ComputeCmd::Courier { action } => match action {
                CourierCmd::Send { file, sender } => {
                    let data = fs::read(&file).expect("read bundle");
                    let store = CourierStore::open("courier.db");
                    let _ = store.send(&data, &sender);
                    std::process::ExitCode::SUCCESS
                }
                CourierCmd::Flush => {
                    let store = CourierStore::open("courier.db");
                    match store.flush(|_| true) {
                        Ok(_) => std::process::ExitCode::SUCCESS,
                        Err(e) => {
                            eprintln!("flush failed: {e}");
                            std::process::ExitCode::FAILURE
                        }
                    }
                }
            },
        },
    };
    code
}

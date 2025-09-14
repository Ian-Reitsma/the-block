#![deny(warnings)]
#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tokio_util::sync::CancellationToken;

use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
#[cfg(feature = "telemetry")]
use tracing::info;
use tracing_chrome::ChromeLayerBuilder;
use tracing_subscriber::{prelude::*, EnvFilter};

#[cfg(feature = "telemetry")]
use the_block::serve_metrics;
use the_block::{
    compute_market::{courier::CourierStore, courier_store::ReceiptStore, matcher},
    generate_keypair,
    rpc::run_rpc_server,
    sign_tx, spawn_purge_loop_thread, Blockchain, RawTxPayload, ShutdownFlag,
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

fn default_db_path() -> String {
    dirs::home_dir()
        .expect("home directory")
        .join(".block")
        .join("db")
        .to_string_lossy()
        .into_owned()
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

        /// Path to RocksDB state database
        #[arg(long, default_value_t = default_db_path())]
        db_path: String,

        /// Directory for chain data
        #[arg(long, default_value = "node-data")]
        data_dir: String,

        /// Log output format: `plain` or `json`
        #[arg(long, default_value = "plain")]
        log_format: String,

        /// Log level directives (e.g. `info`, `mempool=debug`)
        #[arg(long = "log-level", value_name = "LEVEL", num_args = 0.., default_values_t = vec!["info".to_string()])]
        log_level: Vec<String>,

        /// Dry-run compute-market matches (default true)
        #[arg(long, default_value_t = true)]
        dry_run: bool,

        /// Run auto-tuning benchmarks and exit
        #[arg(long, default_value_t = false)]
        auto_tune: bool,

        /// Enable QUIC transport for gossip
        #[arg(long, default_value_t = false)]
        quic: bool,

        /// Enable local mesh networking
        #[arg(long, default_value_t = false)]
        range_boost: bool,

        /// Disable mining and operate as a relay-only node
        #[arg(long, default_value_t = false)]
        relay_only: bool,

        /// Address to serve a status page on
        #[arg(long)]
        status_addr: Option<String>,

        /// Load chain state from snapshot file before starting
        #[arg(long)]
        snapshot: Option<String>,

        /// Port for QUIC listener
        #[arg(long)]
        quic_port: Option<u16>,

        /// Path to QUIC certificate (DER)
        #[arg(long)]
        quic_cert: Option<String>,

        /// Path to QUIC private key (DER)
        #[arg(long)]
        quic_key: Option<String>,

        /// Rotate QUIC certificates after this many days
        #[arg(long)]
        quic_cert_ttl_days: Option<u64>,

        /// Enable runtime profiling and emit Chrome trace to `trace.json`
        #[arg(long, default_value_t = false)]
        profiling: bool,

        /// Country code or path to jurisdiction policy pack
        #[arg(long)]
        jurisdiction: Option<String>,

        /// Enable VM debugging features
        #[arg(long, default_value_t = false)]
        enable_vm_debug: bool,
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
    /// Service badge utilities
    Badge {
        #[command(subcommand)]
        cmd: BadgeCmd,
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
enum BadgeCmd {
    /// Show current badge status
    Status {
        #[arg(long, default_value = "node-data")]
        data_dir: String,
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
            db_path,
            data_dir,
            log_format,
            log_level,
            dry_run,
            auto_tune,
            quic,
            range_boost,
            relay_only,
            status_addr,
            snapshot,
            quic_port: _quic_port,
            quic_cert: _quic_cert,
            quic_key: _quic_key,
            quic_cert_ttl_days: _quic_cert_ttl_days,
            profiling,
            jurisdiction,
            enable_vm_debug,
        } => {
            if auto_tune {
                the_block::telemetry::auto_tune();
                return std::process::ExitCode::SUCCESS;
            }
            the_block::vm::set_vm_debug_enabled(enable_vm_debug);
            let filter = EnvFilter::new(log_level.join(","));
            let (profiler, _chrome) = if profiling {
                let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                    .file("trace.json")
                    .build();
                let subscriber = tracing_subscriber::registry()
                    .with(filter)
                    .with(chrome_layer);
                tracing::subscriber::set_global_default(subscriber).expect("set subscriber");
                (
                    Some(pprof::ProfilerGuard::new(100).expect("profiler")),
                    Some(guard),
                )
            } else {
                let fmt = tracing_subscriber::fmt().with_env_filter(filter);
                if log_format == "json" {
                    fmt.json().init();
                } else {
                    fmt.init();
                }
                (None, None)
            };
            let mut inner = Blockchain::open_with_db(&data_dir, &db_path).expect("open blockchain");
            if let Some(path) = snapshot.as_ref() {
                if let Ok((height, accounts, _root)) =
                    the_block::blockchain::snapshot::load_file(path)
                {
                    inner.accounts = accounts;
                    inner.block_height = height;
                }
            }
            if let Some(arg) = jurisdiction.as_ref() {
                let pack_res = if std::path::Path::new(arg).exists() {
                    jurisdiction::PolicyPack::load(arg)
                } else {
                    jurisdiction::PolicyPack::template(arg).ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "template")
                    })
                };
                match pack_res {
                    Ok(pack) => {
                        inner.config.jurisdiction = Some(pack.region.clone());
                        let _ = the_block::le_portal::record_action(
                            "le_jurisdiction.log",
                            "jurisdiction",
                            &format!("loaded {}", pack.region),
                            &pack.region,
                        );
                    }
                    Err(e) => eprintln!("failed to load jurisdiction pack: {e}"),
                }
            }
            if snapshot_interval != inner.config.snapshot_interval {
                inner.snapshot.set_interval(snapshot_interval);
                inner.config.snapshot_interval = snapshot_interval;
                inner.save_config();
            }
            let bc = Arc::new(Mutex::new(inner));

            let receipt_store = ReceiptStore::open(&format!("{data_dir}/receipts"));
            let match_stop = CancellationToken::new();
            tokio::spawn(matcher::match_loop(
                receipt_store.clone(),
                dry_run,
                match_stop.clone(),
            ));

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

            the_block::range_boost::set_enabled(range_boost);
            if range_boost {
                std::thread::spawn(|| loop {
                    the_block::range_boost::discover_peers();
                    std::thread::sleep(std::time::Duration::from_secs(30));
                });
            }

            let mining = Arc::new(AtomicBool::new(false));
            let (tx, rx) = tokio::sync::oneshot::channel();
            let mut rpc_cfg = bc.lock().unwrap().config.rpc.clone();
            rpc_cfg.relay_only = relay_only;
            let handle = tokio::spawn(run_rpc_server(
                Arc::clone(&bc),
                Arc::clone(&mining),
                rpc_addr.clone(),
                rpc_cfg,
                tx,
            ));
            let rpc_addr = rx.await.expect("rpc addr");
            println!("RPC listening on {rpc_addr}");
            if let Some(addr) = status_addr {
                let bc_status = Arc::clone(&bc);
                tokio::spawn(async move {
                    let addr: std::net::SocketAddr = addr.parse().unwrap();
                    let _ = the_block::web::status::run(addr, bc_status).await;
                });
            }
            if quic {
                #[cfg(feature = "quic")]
                {
                    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
                    use std::path::Path;
                    use std::time::Duration;
                    use the_block::config::QuicConfig;
                    use the_block::net::quic;
                    let port = _quic_port
                        .or_else(|| bc.lock().unwrap().config.quic.as_ref().map(|c| c.port))
                        .unwrap_or(0);
                    let cert_path = _quic_cert.unwrap_or_else(|| {
                        bc.lock()
                            .unwrap()
                            .config
                            .quic
                            .as_ref()
                            .map(|c| c.cert_path.clone())
                            .unwrap_or_else(|| format!("{data_dir}/quic.cert"))
                    });
                    let key_path = _quic_key.unwrap_or_else(|| {
                        bc.lock()
                            .unwrap()
                            .config
                            .quic
                            .as_ref()
                            .map(|c| c.key_path.clone())
                            .unwrap_or_else(|| format!("{data_dir}/quic.key"))
                    });
                    let ttl_days = _quic_cert_ttl_days
                        .or_else(|| {
                            bc.lock()
                                .unwrap()
                                .config
                                .quic
                                .as_ref()
                                .map(|c| c.cert_ttl_days)
                        })
                        .unwrap_or(30);
                    let regen = {
                        let cert_meta = std::fs::metadata(&cert_path).ok();
                        let key_meta = std::fs::metadata(&key_path).ok();
                        match (cert_meta, key_meta) {
                            (Some(cm), Some(km)) => {
                                let uid = nix::unistd::Uid::effective().as_raw();
                                if cm.mode() & 0o777 != 0o600
                                    || km.mode() & 0o777 != 0o600
                                    || cm.uid() != uid
                                    || km.uid() != uid
                                {
                                    panic!("insecure quic cert permissions");
                                }
                                cm.modified()
                                    .ok()
                                    .and_then(|m| m.elapsed().ok())
                                    .map(|d| d > Duration::from_secs(ttl_days * 86_400))
                                    .unwrap_or(true)
                            }
                            _ => true,
                        }
                    };
                    if regen {
                        let cert = rcgen::generate_simple_self_signed(["the-block".to_string()])
                            .expect("generate cert");
                        let cert_der = cert.serialize_der().expect("cert der");
                        let key_der = cert.serialize_private_key_der();
                        let _ = std::fs::create_dir_all(Path::new(&cert_path).parent().unwrap());
                        let mut cf = std::fs::OpenOptions::new()
                            .create(true)
                            .truncate(true)
                            .write(true)
                            .mode(0o600)
                            .open(&cert_path)
                            .expect("write cert");
                        use std::io::Write;
                        cf.write_all(&cert_der).expect("write cert");
                        let mut kf = std::fs::OpenOptions::new()
                            .create(true)
                            .truncate(true)
                            .write(true)
                            .mode(0o600)
                            .open(&key_path)
                            .expect("write key");
                        kf.write_all(&key_der).expect("write key");
                    }
                    let cert_der = std::fs::read(&cert_path).expect("read cert");
                    let key_der = std::fs::read(&key_path).expect("read key");
                    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
                    let _ = quic::listen_with_cert(addr, &cert_der, &key_der).await;
                    {
                        let mut guard = bc.lock().unwrap();
                        guard.config.quic = Some(QuicConfig {
                            port: if port == 0 { addr.port() } else { port },
                            cert_path: cert_path.clone(),
                            key_path: key_path.clone(),
                            cert_ttl_days: ttl_days,
                        });
                        guard.save_config();
                    }
                }
                #[cfg(not(feature = "quic"))]
                eprintln!("quic feature not enabled");
            }
            let _ = handle.await;
            match_stop.cancel();
            the_block::compute_market::price_board::persist();
            let _ = the_block::net::persist_peer_metrics();
            if let Some(g) = profiler {
                if let Ok(report) = g.report().build() {
                    let file = std::fs::File::create("flamegraph.svg").expect("flamegraph");
                    let _ = report.flamegraph(file);
                }
            }
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
                    match store.flush_async(|_| async { true }).await {
                        Ok(_) => std::process::ExitCode::SUCCESS,
                        Err(e) => {
                            eprintln!("flush failed: {e}");
                            std::process::ExitCode::FAILURE
                        }
                    }
                }
            },
        },
        Commands::Badge { cmd } => match cmd {
            BadgeCmd::Status { data_dir } => {
                let mut chain = Blockchain::open(&data_dir).expect("open blockchain");
                chain.check_badges();
                let (active, last_mint, last_burn) = chain.badge_status();
                println!(
                    "active: {active}, last_mint: {:?}, last_burn: {:?}",
                    last_mint, last_burn
                );
                std::process::ExitCode::SUCCESS
            }
        },
    };
    code
}

#![deny(warnings)]
#![allow(clippy::expect_used)]

use runtime::sync::CancellationToken;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::signatures::ed25519::SigningKey;
use sys::paths;
use sys::process;
use tracing_chrome::ChromeLayerBuilder;
use tracing_subscriber::{prelude::*, util::SubscriberInitExt, EnvFilter};

use the_block::config::OverlayBackend;
#[cfg(feature = "telemetry")]
use the_block::serve_metrics;
use the_block::{
    compute_market::{courier::CourierStore, courier_store::ReceiptStore, matcher},
    generate_keypair,
    rpc::run_rpc_server,
    sign_tx, spawn_purge_loop_thread, Blockchain, RawTxPayload, ShutdownFlag,
};

mod cli_support;
use cli_support::{collect_args, parse_matches};

fn key_dir() -> PathBuf {
    paths::home_dir()
        .expect("home directory")
        .join(".the_block")
        .join("keys")
}

fn key_path(id: &str) -> PathBuf {
    key_dir().join(format!("{id}.pem"))
}

fn write_pem(path: &Path, sk: &SigningKey) -> std::io::Result<()> {
    use base64_fp::encode_standard;
    let pem = format!(
        "-----BEGIN ED25519 PRIVATE KEY-----\n{}\n-----END ED25519 PRIVATE KEY-----\n",
        encode_standard(&sk.to_bytes())
    );
    fs::write(path, pem)
}

fn read_pem(src: &str) -> std::io::Result<SigningKey> {
    use base64_fp::decode_standard;
    let b64: String = src.lines().filter(|l| !l.starts_with("---")).collect();
    let bytes = decode_standard(&b64)
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
    paths::home_dir()
        .expect("home directory")
        .join(".block")
        .join("db")
        .to_string_lossy()
        .into_owned()
}

fn default_language_for_region(region: &str) -> &'static str {
    match region {
        "US" => "en-US",
        "EU" => "en-GB",
        _ => "en",
    }
}

fn policy_pack_language(pack: &jurisdiction::PolicyPack) -> String {
    pack.features
        .iter()
        .find_map(|feature| {
            feature
                .strip_prefix("language:")
                .map(|code| code.to_string())
        })
        .unwrap_or_else(|| default_language_for_region(&pack.region).to_string())
}

#[derive(Debug)]
struct Cli {
    command: Commands,
}

#[derive(Clone, Debug)]
enum OverlayBackendArg {
    Inhouse,
    Stub,
}

impl From<OverlayBackendArg> for OverlayBackend {
    fn from(arg: OverlayBackendArg) -> Self {
        match arg {
            OverlayBackendArg::Inhouse => OverlayBackend::Inhouse,
            OverlayBackendArg::Stub => OverlayBackend::Stub,
        }
    }
}

impl std::str::FromStr for OverlayBackendArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "inhouse" => Ok(Self::Inhouse),
            "stub" => Ok(Self::Stub),
            other => Err(format!("invalid overlay backend '{other}'")),
        }
    }
}

#[derive(Debug)]
enum Commands {
    /// Run a full node with JSON-RPC controls
    Run {
        /// Address to bind the JSON-RPC server to
        rpc_addr: String,

        /// Seconds between mempool purge sweeps (0 to disable)
        mempool_purge_interval: u64,

        /// Interval in blocks between full snapshots
        snapshot_interval: u64,

        /// Expose Prometheus metrics on this address (requires `--features telemetry`)
        metrics_addr: Option<String>,

        /// Path to RocksDB state database
        db_path: String,

        /// Directory for chain data
        data_dir: String,

        /// Log output format: `plain` or `json`
        log_format: String,

        /// Log level directives (e.g. `info`, `mempool=debug`)
        log_level: Vec<String>,

        /// Dry-run compute-market matches (default true)
        dry_run: bool,

        /// Run auto-tuning benchmarks and exit
        auto_tune: bool,

        /// Enable QUIC transport for gossip
        quic: bool,

        /// Enable local mesh networking
        range_boost: bool,

        /// Disable mining and operate as a relay-only node
        relay_only: bool,

        /// Address to serve a status page on
        status_addr: Option<String>,

        /// Load chain state from snapshot file before starting
        snapshot: Option<String>,

        /// Port for QUIC listener
        quic_port: Option<u16>,

        /// Path to QUIC certificate (DER)
        quic_cert: Option<String>,

        /// Path to QUIC private key (DER)
        quic_key: Option<String>,

        /// Rotate QUIC certificates after this many days
        quic_cert_ttl_days: Option<u64>,

        /// Enable runtime profiling and emit Chrome trace to `trace.json`
        profiling: bool,

        /// Country code or path to jurisdiction policy pack
        jurisdiction: Option<String>,

        /// Overlay backend for peer discovery and uptime tracking
        overlay_backend: Option<OverlayBackendArg>,

        /// Enable VM debugging features
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

#[derive(Debug)]
enum ComputeCmd {
    /// Courier receipt operations
    Courier { action: CourierCmd },
}

#[derive(Debug)]
enum BadgeCmd {
    /// Show current badge status
    Status { data_dir: String },
}

#[derive(Debug)]
enum CourierCmd {
    /// Send a bundle and store a courier receipt
    Send { file: String, sender: String },
    /// Flush stored receipts
    Flush,
}

fn build_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("node"),
        "node",
        "Run a basic node or manage wallet keys",
    )
    .subcommand(build_run_command())
    .subcommand(
        CommandBuilder::new(
            CommandId("node.generate_key"),
            "generate-key",
            "Generate a new keypair saved under ~/.the_block/keys/<key_id>.pem",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "key_id",
            "Key identifier",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("node.import_key"),
            "import-key",
            "Import an existing PEM-encoded key file",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "file",
            "Path to PEM file",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("node.show_address"),
            "show-address",
            "Show the hex address for the given key id",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "key_id",
            "Key identifier",
        )))
        .build(),
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("node.sign_tx"),
            "sign-tx",
            "Sign a transaction JSON payload with the given key",
        )
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "key_id",
            "Key identifier",
        )))
        .arg(ArgSpec::Positional(PositionalSpec::new(
            "tx_json",
            "Transaction JSON payload",
        )))
        .build(),
    )
    .subcommand(build_compute_command())
    .subcommand(build_badge_command())
    .build()
}

fn build_run_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("node.run"),
        "run",
        "Run a full node with JSON-RPC controls",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "rpc_addr",
            "rpc-addr",
            "Address to bind the JSON-RPC server to",
        )
        .default("127.0.0.1:3030"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "mempool_purge_interval",
            "mempool-purge-interval",
            "Seconds between mempool purge sweeps (0 to disable)",
        )
        .default("0"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "snapshot_interval",
            "snapshot-interval",
            "Interval in blocks between full snapshots",
        )
        .default("600"),
    ))
    .arg(ArgSpec::Option(OptionSpec::new(
        "metrics_addr",
        "metrics-addr",
        "Expose Prometheus metrics on this address",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "db_path",
        "db-path",
        "Path to RocksDB state database",
    )))
    .arg(ArgSpec::Option(
        OptionSpec::new("data_dir", "data-dir", "Directory for chain data").default("node-data"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("log_format", "log-format", "Log output format").default("plain"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new("log_level", "log-level", "Log level directives")
            .multiple(true)
            .default("info"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "dry_run",
            "dry-run",
            "Dry-run compute-market matches (default true)",
        )
        .default("true"),
    ))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "auto_tune",
        "auto-tune",
        "Run auto-tuning benchmarks and exit",
    )))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "quic",
        "quic",
        "Enable QUIC transport for gossip",
    )))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "range_boost",
        "range-boost",
        "Enable local mesh networking",
    )))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "relay_only",
        "relay-only",
        "Disable mining and operate as a relay-only node",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "status_addr",
        "status-addr",
        "Address to serve a status page on",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "snapshot",
        "snapshot",
        "Load chain state from snapshot file before starting",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "quic_port",
        "quic-port",
        "Port for QUIC listener",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "quic_cert",
        "quic-cert",
        "Path to QUIC certificate (DER)",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "quic_key",
        "quic-key",
        "Path to QUIC private key (DER)",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "quic_cert_ttl_days",
        "quic-cert-ttl-days",
        "Rotate QUIC certificates after this many days",
    )))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "profiling",
        "profiling",
        "Enable runtime profiling and emit Chrome trace to trace.json",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "jurisdiction",
        "jurisdiction",
        "Country code or path to jurisdiction policy pack",
    )))
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "overlay_backend",
            "overlay-backend",
            "Overlay backend for peer discovery and uptime tracking",
        )
        .value_enum(&["inhouse", "stub"]),
    ))
    .arg(ArgSpec::Flag(FlagSpec::new(
        "enable_vm_debug",
        "enable-vm-debug",
        "Enable VM debugging features",
    )))
    .build()
}

fn build_compute_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("node.compute"),
        "compute",
        "Compute-related utilities",
    )
    .subcommand(
        CommandBuilder::new(
            CommandId("node.compute.courier"),
            "courier",
            "Courier receipt operations",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("node.compute.courier.send"),
                "send",
                "Send a bundle and store a courier receipt",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "file",
                "Path to bundle file",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "sender",
                "Sender identifier",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("node.compute.courier.flush"),
                "flush",
                "Flush stored receipts",
            )
            .build(),
        )
        .build(),
    )
    .build()
}

fn build_badge_command() -> CliCommand {
    CommandBuilder::new(CommandId("node.badge"), "badge", "Service badge utilities")
        .subcommand(
            CommandBuilder::new(
                CommandId("node.badge.status"),
                "status",
                "Show current badge status",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "data_dir",
                    "data-dir",
                    "Data directory containing badge information",
                )
                .default("node-data"),
            ))
            .build(),
        )
        .build()
}

fn build_cli(matches: Matches) -> Result<Cli, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    let command = match sub {
        "run" => parse_run(sub_matches)?,
        "generate-key" => Commands::GenerateKey {
            key_id: require_positional(sub_matches, "key_id")?,
        },
        "import-key" => Commands::ImportKey {
            file: require_positional(sub_matches, "file")?,
        },
        "show-address" => Commands::ShowAddress {
            key_id: require_positional(sub_matches, "key_id")?,
        },
        "sign-tx" => Commands::SignTx {
            key_id: require_positional(sub_matches, "key_id")?,
            tx_json: require_positional(sub_matches, "tx_json")?,
        },
        "compute" => Commands::Compute {
            cmd: parse_compute(sub_matches)?,
        },
        "badge" => Commands::Badge {
            cmd: parse_badge(sub_matches)?,
        },
        other => return Err(format!("unknown subcommand '{other}'")),
    };

    Ok(Cli { command })
}

fn parse_run(matches: &Matches) -> Result<Commands, String> {
    let rpc_addr = matches
        .get_string("rpc_addr")
        .unwrap_or_else(|| "127.0.0.1:3030".to_string());
    let mempool_purge_interval = parse_u64_option(matches, "mempool_purge_interval", 0)?;
    let snapshot_interval = parse_u64_option(matches, "snapshot_interval", 600)?;
    let metrics_addr = matches.get_string("metrics_addr");
    let db_path = matches
        .get_string("db_path")
        .unwrap_or_else(default_db_path);
    let data_dir = matches
        .get_string("data_dir")
        .unwrap_or_else(|| "node-data".to_string());
    let log_format = matches
        .get_string("log_format")
        .unwrap_or_else(|| "plain".to_string());
    let mut log_level = matches.get_strings("log_level");
    if log_level.is_empty() {
        log_level.push("info".to_string());
    }
    let dry_run = parse_bool_option(matches, "dry_run", true)?;
    let auto_tune = matches.get_flag("auto_tune");
    let quic = matches.get_flag("quic");
    let range_boost = matches.get_flag("range_boost");
    let relay_only = matches.get_flag("relay_only");
    let status_addr = matches.get_string("status_addr");
    let snapshot = matches.get_string("snapshot");
    let quic_port = parse_optional_u16(matches, "quic_port")?;
    let quic_cert = matches.get_string("quic_cert");
    let quic_key = matches.get_string("quic_key");
    let quic_cert_ttl_days = parse_optional_u64(matches, "quic_cert_ttl_days")?;
    let profiling = matches.get_flag("profiling");
    let jurisdiction = matches.get_string("jurisdiction");
    let overlay_backend = matches
        .get_string("overlay_backend")
        .map(|value| value.parse::<OverlayBackendArg>())
        .transpose()?;
    let enable_vm_debug = matches.get_flag("enable_vm_debug");

    Ok(Commands::Run {
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
        quic_port,
        quic_cert,
        quic_key,
        quic_cert_ttl_days,
        profiling,
        jurisdiction,
        overlay_backend,
        enable_vm_debug,
    })
}

fn parse_compute(matches: &Matches) -> Result<ComputeCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing compute subcommand".to_string())?;

    match sub {
        "courier" => Ok(ComputeCmd::Courier {
            action: parse_courier(sub_matches)?,
        }),
        other => Err(format!("unknown compute subcommand '{other}'")),
    }
}

fn parse_courier(matches: &Matches) -> Result<CourierCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing courier action".to_string())?;

    match sub {
        "send" => Ok(CourierCmd::Send {
            file: require_positional(sub_matches, "file")?,
            sender: require_positional(sub_matches, "sender")?,
        }),
        "flush" => Ok(CourierCmd::Flush),
        other => Err(format!("unknown courier action '{other}'")),
    }
}

fn parse_badge(matches: &Matches) -> Result<BadgeCmd, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing badge subcommand".to_string())?;

    match sub {
        "status" => Ok(BadgeCmd::Status {
            data_dir: sub_matches
                .get_string("data_dir")
                .unwrap_or_else(|| "node-data".to_string()),
        }),
        other => Err(format!("unknown badge subcommand '{other}'")),
    }
}

fn require_positional(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| format!("missing argument '{name}'"))
}

fn parse_u64_option(matches: &Matches, name: &str, default: u64) -> Result<u64, String> {
    matches
        .get_string(name)
        .unwrap_or_else(|| default.to_string())
        .parse::<u64>()
        .map_err(|err| format!("invalid value for {name}: {err}"))
}

fn parse_optional_u16(matches: &Matches, name: &str) -> Result<Option<u16>, String> {
    matches
        .get_string(name)
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|err| format!("invalid {name}: {err}"))
        })
        .transpose()
}

fn parse_optional_u64(matches: &Matches, name: &str) -> Result<Option<u64>, String> {
    matches
        .get_string(name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| format!("invalid {name}: {err}"))
        })
        .transpose()
}

fn parse_bool_option(matches: &Matches, name: &str, default: bool) -> Result<bool, String> {
    let raw = matches
        .get_string(name)
        .unwrap_or_else(|| default.to_string());
    match raw.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        other => Err(format!("invalid boolean for {name}: {other}")),
    }
}

fn rollback_and_exit(reason: &str) -> std::process::ExitCode {
    eprintln!("startup aborted: {reason}");
    if let Err(rb) = the_block::update::rollback_failed_startup() {
        eprintln!("rollback attempt failed: {rb}");
    } else {
        eprintln!("previous binary restored from TB_PREVIOUS_BINARY");
    }
    std::process::ExitCode::FAILURE
}

fn main() -> std::process::ExitCode {
    runtime::block_on(async_main())
}

async fn async_main() -> std::process::ExitCode {
    let command = build_command();
    let (bin, args) = collect_args("node");
    let matches = match parse_matches(&command, &bin, args) {
        Some(matches) => matches,
        None => return std::process::ExitCode::SUCCESS,
    };

    let cli = match build_cli(matches) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{err}");
            return std::process::ExitCode::from(2);
        }
    };
    // Verify build provenance at startup.
    if !the_block::provenance::verify_self() {
        return rollback_and_exit("binary provenance verification failed");
    }
    if let Err(err) = the_block::governance::ensure_release_authorized(env!("BUILD_BIN_HASH")) {
        return rollback_and_exit(err.as_str());
    }
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
            overlay_backend,
            enable_vm_debug,
        } => {
            if auto_tune {
                #[cfg(feature = "telemetry")]
                {
                    the_block::telemetry::auto_tune();
                    return std::process::ExitCode::SUCCESS;
                }
                #[cfg(not(feature = "telemetry"))]
                {
                    eprintln!("telemetry feature not enabled; --auto-tune unavailable");
                    return std::process::ExitCode::FAILURE;
                }
            }
            the_block::vm::set_vm_debug_enabled(enable_vm_debug);
            #[cfg(feature = "telemetry")]
            the_block::telemetry::init_wrapper_metrics();
            let filter = EnvFilter::new(log_level.join(","));
            let (profiler, _chrome) = if profiling {
                let (chrome_layer, guard) = ChromeLayerBuilder::new().file("trace.json").build();
                tracing_subscriber::registry()
                    .with(filter)
                    .with(chrome_layer)
                    .try_init()
                    .expect("set subscriber");
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
                        let language = policy_pack_language(&pack);
                        let _ = the_block::le_portal::record_action(
                            "le_jurisdiction.log",
                            "jurisdiction",
                            &format!("loaded {}", pack.region),
                            &pack.region,
                            &language,
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

            let overlay_choice = overlay_backend.map(OverlayBackend::from);
            let overlay_cfg = {
                let mut guard = bc.lock().unwrap();
                if let Some(choice) = overlay_choice {
                    if guard.config.overlay.backend != choice {
                        guard.config.overlay.backend = choice;
                        guard.save_config();
                    }
                }
                guard.config.overlay.clone()
            };
            the_block::net::configure_overlay(&overlay_cfg);
            if let Err(err) = the_block::config::ensure_overlay_sanity(&overlay_cfg) {
                eprintln!("overlay_sanity_failed: {err}");
                return std::process::ExitCode::FAILURE;
            }

            let receipt_store = ReceiptStore::open(&format!("{data_dir}/receipts"));
            let match_stop = CancellationToken::new();
            runtime::spawn(matcher::match_loop(
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
            let (tx, rx) = runtime::sync::oneshot::channel();
            let mut rpc_cfg = bc.lock().unwrap().config.rpc.clone();
            rpc_cfg.relay_only = relay_only;
            let handle = runtime::spawn(run_rpc_server(
                Arc::clone(&bc),
                Arc::clone(&mining),
                rpc_addr.clone(),
                rpc_cfg,
                tx,
            ));
            let rpc_addr = rx.await.expect("rpc addr");
            println!("RPC listening on {rpc_addr}");
            #[cfg(feature = "gateway")]
            if let Some(addr) = status_addr.clone() {
                let bc_status = Arc::clone(&bc);
                runtime::spawn(async move {
                    let addr: std::net::SocketAddr = addr.parse().unwrap();
                    let _ = the_block::web::status::run(addr, bc_status).await;
                });
            }
            #[cfg(not(feature = "gateway"))]
            if status_addr.is_some() {
                eprintln!("gateway feature not enabled; status server unavailable");
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
                                let uid = process::effective_uid().unwrap_or(0);
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

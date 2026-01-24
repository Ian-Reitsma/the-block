use std::{env, error::Error, net::SocketAddr};

use diagnostics::anyhow::Result;
use runtime::{self, sync::mpsc};
use the_block::{
    config, gateway::stake::DnsStakeTable, gateway::tls::build_tls_config, web::gateway, ReadAck,
};

struct ServiceArgs {
    listen: SocketAddr,
    config_dir: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    runtime::block_on(async_main(args)).map_err(|err| err.into())
}

fn parse_args() -> Result<ServiceArgs, String> {
    let mut listen = None;
    let mut config_dir = None;
    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--listen" => {
                listen = Some(
                    iter.next()
                        .ok_or_else(|| "missing value for --listen".to_string())?,
                );
            }
            "--config-dir" => {
                config_dir = Some(
                    iter.next()
                        .ok_or_else(|| "missing value for --config-dir".to_string())?,
                );
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument '{arg}'")),
        }
    }
    let listen_str = listen
        .or_else(|| env::var("TB_GATEWAY_LISTEN").ok())
        .unwrap_or_else(|| "0.0.0.0:9000".to_string());
    let listen = listen_str
        .parse::<SocketAddr>()
        .map_err(|err| format!("invalid listen address '{listen_str}': {err}"))?;
    let config_dir = config_dir
        .or_else(|| env::var("TB_GATEWAY_CONFIG_DIR").ok())
        .unwrap_or_else(|| "config".to_string());
    Ok(ServiceArgs { listen, config_dir })
}

fn print_usage() {
    eprintln!("gateway-service [--listen LISTEN] [--config-dir DIR]");
    eprintln!("  --listen        Address the gateway server binds to (defaults to 0.0.0.0:9000)");
    eprintln!(
        "  --config-dir    Directory with node configs (defaults to 'config' or TB_GATEWAY_CONFIG_DIR)"
    );
}

async fn async_main(args: ServiceArgs) -> Result<()> {
    let _cfg = config::NodeConfig::load(&args.config_dir);
    config::watch(&args.config_dir);
    let tls_config = build_tls_config(None, None, None, None)?;
    let (read_tx, mut read_rx) = mpsc::channel::<ReadAck>(1024);
    runtime::spawn(async move {
        while let Some(_) = read_rx.recv().await {
            // Read acknowledgements are currently not persisted by this service binary.
        }
    });
    gateway::run(
        args.listen,
        DnsStakeTable::new(),
        read_tx,
        None,
        None,
        tls_config,
        None,
    )
    .await
}

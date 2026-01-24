use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    net::SocketAddr,
    path::PathBuf,
};

use diagnostics::anyhow::Result;
use foundation_serialization::json;
use runtime::{self, sync::mpsc};
use the_block::{
    config,
    gateway::{
        stake::{self, DnsStakeTable},
        tls::build_tls_config,
    },
    web::gateway,
    ReadAck,
};

const DEFAULT_ACK_DIR: &str = "gateway_acks";

struct ReadAckPersister {
    base: PathBuf,
    current_epoch: Option<u64>,
    writer: Option<BufWriter<std::fs::File>>,
}

impl ReadAckPersister {
    fn new(base: PathBuf) -> Self {
        Self {
            base,
            current_epoch: None,
            writer: None,
        }
    }

    fn persist(&mut self, ack: ReadAck) -> io::Result<()> {
        let epoch = ack.ts / 3600;
        let writer = self.roll_writer(epoch)?;
        let payload =
            json::to_string(&ack).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        writer.write_all(payload.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(())
    }

    fn roll_writer(&mut self, epoch: u64) -> io::Result<&mut BufWriter<std::fs::File>> {
        if self.current_epoch != Some(epoch) || self.writer.is_none() {
            fs::create_dir_all(&self.base)?;
            let file_path = self.base.join(format!("{epoch}.jsonl"));
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_path)?;
            self.writer = Some(BufWriter::new(file));
            self.current_epoch = Some(epoch);
        }
        Ok(self.writer.as_mut().expect("writer initialized"))
    }
}

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
    let ack_dir = PathBuf::from(
        env::var("TB_GATEWAY_ACK_DIR").unwrap_or_else(|_| DEFAULT_ACK_DIR.to_string()),
    );
    let persister = ReadAckPersister::new(ack_dir);
    runtime::spawn({
        let mut persister = persister;
        async move {
            while let Some(ack) = read_rx.recv().await {
                if let Err(err) = persister.persist(ack) {
                    eprintln!("gateway-service: failed to persist read ack: {err}");
                }
            }
        }
    });
    gateway::run(
        args.listen,
        stake::with_env_overrides(DnsStakeTable::new()),
        read_tx,
        None,
        None,
        tls_config,
        None,
    )
    .await
}

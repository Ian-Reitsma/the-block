use std::{env, error::Error, net::SocketAddr, process, sync::Arc};

use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use diagnostics::anyhow::{anyhow, Result};
use httpd::ServerTlsConfig;
use runtime::{self, sync::mpsc};
use the_block::{
    config,
    gateway::dns,
    net,
    web::gateway::{self, ResolverConfig, StakeTable},
    ReadAck,
};

mod cli_support;
use cli_support::{collect_args, parse_matches};

#[derive(Debug)]
struct GatewayArgs {
    listen: SocketAddr,
    config_dir: String,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    tls_client_ca: Option<String>,
    tls_client_ca_optional: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let command = build_command();
    let (bin, args) = collect_args("gateway");
    let matches = match parse_matches(&command, &bin, args) {
        Some(matches) => matches,
        None => return Ok(()),
    };
    let gateway_args = match parse_args(matches) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };
    runtime::block_on(async_main(gateway_args)).map_err(|err| err.into())
}

fn build_command() -> CliCommand {
    CommandBuilder::new(
        CommandId("gateway"),
        "gateway",
        "Run the The-Block HTTP gateway service",
    )
    .arg(ArgSpec::Option(
        OptionSpec::new("listen", "listen", "Address to bind the gateway server to")
            .default("0.0.0.0:9000"),
    ))
    .arg(ArgSpec::Option(
        OptionSpec::new(
            "config_dir",
            "config-dir",
            "Directory holding node configuration",
        )
        .default("config"),
    ))
    .arg(ArgSpec::Option(OptionSpec::new(
        "tls_cert",
        "tls-cert",
        "Path to TLS leaf certificate (PEM)",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "tls_key",
        "tls-key",
        "Path to TLS private key (PEM)",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "tls_client_ca",
        "tls-client-ca",
        "Path to required client CA bundle (PEM)",
    )))
    .arg(ArgSpec::Option(OptionSpec::new(
        "tls_client_ca_optional",
        "tls-client-ca-optional",
        "Path to optional client CA bundle (PEM)",
    )))
    .build()
}

fn parse_args(matches: Matches) -> Result<GatewayArgs, String> {
    let listen_str = matches
        .get_string("listen")
        .unwrap_or_else(|| "0.0.0.0:9000".to_string());
    let listen = listen_str
        .parse::<SocketAddr>()
        .map_err(|err| format!("invalid listen address '{listen_str}': {err}"))?;
    let config_dir = matches
        .get_string("config_dir")
        .unwrap_or_else(|| "config".to_string());
    Ok(GatewayArgs {
        listen,
        config_dir,
        tls_cert: matches.get_string("tls_cert"),
        tls_key: matches.get_string("tls_key"),
        tls_client_ca: matches.get_string("tls_client_ca"),
        tls_client_ca_optional: matches.get_string("tls_client_ca_optional"),
    })
}

async fn async_main(args: GatewayArgs) -> Result<()> {
    let _cfg = config::NodeConfig::load(&args.config_dir);
    config::watch(&args.config_dir);
    let tls_config = build_tls_config(&args)?;
    let listener =
        net::listener::bind_runtime("gateway", "gateway_listener_bind_failed", args.listen).await?;
    let (read_tx, mut read_rx) = mpsc::channel::<ReadAck>(1024);
    runtime::spawn(async move {
        while let Some(_) = read_rx.recv().await {
            // Read acknowledgements are currently not persisted in the gateway-only binary.
        }
    });
    let stake_table = Arc::new(DnsStakeTable);
    gateway::run_listener(
        listener,
        stake_table,
        read_tx,
        None,
        None,
        tls_config,
        ResolverConfig::from_env(),
    )
    .await
}

fn build_tls_config(args: &GatewayArgs) -> Result<Option<ServerTlsConfig>> {
    let cert = args
        .tls_cert
        .as_ref()
        .cloned()
        .or_else(|| env::var("TB_GATEWAY_TLS_CERT").ok());
    let key = args
        .tls_key
        .as_ref()
        .cloned()
        .or_else(|| env::var("TB_GATEWAY_TLS_KEY").ok());
    let client_ca = args
        .tls_client_ca
        .as_ref()
        .cloned()
        .or_else(|| env::var("TB_GATEWAY_TLS_CLIENT_CA").ok());
    let client_ca_optional = args
        .tls_client_ca_optional
        .as_ref()
        .cloned()
        .or_else(|| env::var("TB_GATEWAY_TLS_CLIENT_CA_OPTIONAL").ok());

    if cert.is_none() && key.is_none() && client_ca.is_none() && client_ca_optional.is_none() {
        return Ok(None);
    }

    let cert = cert.ok_or_else(|| {
        anyhow!("tls identity requires both a certificate and private key; missing certificate")
    })?;
    let key = key.ok_or_else(|| {
        anyhow!("tls identity requires both a certificate and private key; missing private key")
    })?;

    if client_ca.is_some() && client_ca_optional.is_some() {
        return Err(anyhow!(
            "only one of TB_GATEWAY_TLS_CLIENT_CA or TB_GATEWAY_TLS_CLIENT_CA_OPTIONAL may be set"
        ));
    }

    let config = if let Some(ca) = client_ca {
        ServerTlsConfig::from_identity_files_with_client_auth(&cert, &key, ca)?
    } else if let Some(ca) = client_ca_optional {
        ServerTlsConfig::from_identity_files_with_optional_client_auth(&cert, &key, ca)?
    } else {
        ServerTlsConfig::from_identity_files(&cert, &key)?
    };
    Ok(Some(config))
}

struct DnsStakeTable;

impl StakeTable for DnsStakeTable {
    fn has_stake(&self, domain: &str) -> bool {
        dns::domain_has_stake(domain)
    }
}

use crate::rpc::RpcClient;
use anyhow::{anyhow, Context, Result};
use clap::Subcommand;
use cli_core::ConfigReader;
use serde_json::json;
use std::{path::PathBuf, process};

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Trigger config reload
    Reload {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Display configuration values parsed by the in-house reader
    Show {
        /// Optional configuration file path (defaults to ~/.the_block/config.cfg)
        #[arg(long)]
        file: Option<PathBuf>,
        /// Restrict output to a single key
        #[arg(long)]
        key: Option<String>,
    },
}

pub fn handle(cmd: ConfigCmd) {
    match cmd {
        ConfigCmd::Reload { url } => reload(url),
        ConfigCmd::Show { file, key } => {
            if let Err(err) = show(file, key) {
                eprintln!("{err}");
                process::exit(2);
            }
        }
    }
}

pub fn reload(url: String) {
    let client = RpcClient::from_env();
    #[derive(serde::Serialize)]
    struct Payload<'a> {
        jsonrpc: &'static str,
        id: u32,
        method: &'static str,
        params: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth: Option<&'a str>,
    }
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "config.reload",
        params: json!({}),
        auth: None,
    };
    if let Ok(resp) = client.call(&url, &payload) {
        if let Ok(text) = resp.text() {
            println!("{}", text);
        }
    }
}

pub fn show(file: Option<PathBuf>, key: Option<String>) -> Result<()> {
    let path = file.unwrap_or_else(default_config_path);
    let reader = ConfigReader::load_file(&path)
        .with_context(|| format!("failed to load {}", path.display()))?;

    if let Some(key) = key {
        match reader.get(&key) {
            Some(value) => println!("{value}"),
            None => return Err(anyhow!("key '{key}' not found in {}", path.display())),
        }
    } else {
        for (key, value) in reader.entries() {
            println!("{key}={value}");
        }
    }

    Ok(())
}

fn default_config_path() -> PathBuf {
    if let Some(mut dir) = dirs::home_dir() {
        dir.push(".the_block");
        dir.push("config.cfg");
        dir
    } else {
        PathBuf::from("config.cfg")
    }
}

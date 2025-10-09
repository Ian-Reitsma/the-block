use crate::parse_utils::{optional_path, take_string};
use crate::rpc::RpcClient;
use anyhow::{anyhow, Context, Result};
use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
    ConfigReader,
};
use std::{path::PathBuf, process};

pub enum ConfigCmd {
    /// Trigger config reload
    Reload { url: String },
    /// Display configuration values parsed by the in-house reader
    Show {
        /// Optional configuration file path (defaults to ~/.the_block/config.cfg)
        file: Option<PathBuf>,
        /// Restrict output to a single key
        key: Option<String>,
    },
}

impl ConfigCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("config"), "config", "Config utilities")
            .subcommand(
                CommandBuilder::new(
                    CommandId("config.reload"),
                    "reload",
                    "Trigger config reload",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("url", "url", "RPC endpoint").default("http://localhost:26658"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("config.show"), "show", "Display configuration")
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "file",
                        "file",
                        "Configuration file path",
                    )))
                    .arg(ArgSpec::Option(OptionSpec::new(
                        "key",
                        "key",
                        "Restrict output to a single key",
                    )))
                    .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'config'".to_string())?;

        match name {
            "reload" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(ConfigCmd::Reload { url })
            }
            "show" => Ok(ConfigCmd::Show {
                file: optional_path(sub_matches, "file"),
                key: take_string(sub_matches, "key"),
            }),
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
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
    #[derive(Serialize)]
    struct Payload<'a> {
        jsonrpc: &'static str,
        id: u32,
        method: &'static str,
        params: foundation_serialization::json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth: Option<&'a str>,
    }
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "config.reload",
        params: foundation_serialization::json!({}),
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
    if let Some(mut dir) = sys::paths::home_dir() {
        dir.push(".the_block");
        dir.push("config.cfg");
        dir
    } else {
        PathBuf::from("config.cfg")
    }
}

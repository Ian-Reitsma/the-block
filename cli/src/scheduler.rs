use crate::parse_utils::take_string;
use crate::rpc::RpcClient;
use cli_core::{
    arg::ArgSpec,
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};

pub enum SchedulerCmd {
    /// Show scheduler queue depths and weights
    Stats { url: String },
}

impl SchedulerCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("scheduler"), "scheduler", "Scheduler diagnostics")
            .subcommand(
                CommandBuilder::new(
                    CommandId("scheduler.stats"),
                    "stats",
                    "Show scheduler queue depths and weights",
                )
                .arg(ArgSpec::Option(
                    cli_core::arg::OptionSpec::new("url", "url", "RPC endpoint")
                        .default("http://localhost:26658"),
                ))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'scheduler'".to_string())?;

        match name {
            "stats" => {
                let url = take_string(sub_matches, "url")
                    .unwrap_or_else(|| "http://localhost:26658".to_string());
                Ok(SchedulerCmd::Stats { url })
            }
            other => Err(format!("unknown subcommand '{other}' for 'scheduler'")),
        }
    }
}

pub fn handle(cmd: SchedulerCmd) {
    match cmd {
        SchedulerCmd::Stats { url } => {
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
                method: "scheduler.stats",
                params: foundation_serialization::json!({}),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
    }
}

pub mod ai;
pub mod bridge;
pub mod codec_helpers;
pub mod compute;
#[cfg(not(doc))]
pub mod explorer;
pub mod gov;
pub mod http_client;
pub mod identity;
pub mod json_helpers;
pub mod light_client;
pub mod parse_utils;
pub mod remediation;
pub mod rpc;
pub mod tls;
pub mod tx;
pub mod wallet;
pub mod wasm;

#[cfg(doc)]
pub mod explorer {
    use cli_core::{
        command::{Command, CommandBuilder, CommandId},
        parse::Matches,
    };
    use std::io::Write;

    #[derive(Clone, Copy, Debug)]
    pub enum ExplorerCmd {
        ReleaseHistory,
        BlockPayouts,
        SyncProofs,
    }

    impl ExplorerCmd {
        pub fn command() -> Command {
            CommandBuilder::new(CommandId("explorer"), "explorer", "Explorer tooling (stub)")
                .build()
        }

        pub fn from_matches(_: &Matches) -> Result<Self, String> {
            Err("explorer tooling is unavailable during documentation builds".into())
        }
    }

    pub fn handle(_cmd: ExplorerCmd) {}

    pub fn handle_with_writer(_: ExplorerCmd, _: &mut impl Write) -> Result<(), String> {
        Err("explorer tooling is unavailable during documentation builds".into())
    }
}

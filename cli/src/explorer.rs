use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use std::cmp::Reverse;
use the_block::governance::{controller, GovStore};

pub enum ExplorerCmd {
    /// Show release history with optional filters
    ReleaseHistory {
        state: String,
        proposer: Option<String>,
        start_epoch: Option<u64>,
        end_epoch: Option<u64>,
        page: usize,
        page_size: usize,
    },
}

impl ExplorerCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("explorer"), "explorer", "Explorer utilities")
            .subcommand(
                CommandBuilder::new(
                    CommandId("explorer.release_history"),
                    "release-history",
                    "Show release history with optional filters",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("state", "state", "Governance database path")
                        .default("governance_db"),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "proposer",
                    "proposer",
                    "Filter by proposer",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "start_epoch",
                    "start-epoch",
                    "Minimum activation epoch",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "end_epoch",
                    "end-epoch",
                    "Maximum activation epoch",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page", "page", "Page index (0-based)").default("0"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("page_size", "page-size", "Page size").default("20"),
                ))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'explorer'".to_string())?;

        match name {
            "release-history" => {
                let state = sub_matches
                    .get_string("state")
                    .unwrap_or_else(|| "governance_db".to_string());
                let proposer = sub_matches.get_string("proposer");
                let start_epoch = match sub_matches.get_string("start_epoch") {
                    Some(v) => Some(
                        v.parse::<u64>()
                            .map_err(|err| format!("invalid value for '--start-epoch': {err}"))?,
                    ),
                    None => None,
                };
                let end_epoch = match sub_matches.get_string("end_epoch") {
                    Some(v) => Some(
                        v.parse::<u64>()
                            .map_err(|err| format!("invalid value for '--end-epoch': {err}"))?,
                    ),
                    None => None,
                };
                let page = sub_matches
                    .get_string("page")
                    .unwrap_or_else(|| "0".to_string())
                    .parse::<usize>()
                    .map_err(|err| format!("invalid value for '--page': {err}"))?;
                let page_size = sub_matches
                    .get_string("page_size")
                    .unwrap_or_else(|| "20".to_string())
                    .parse::<usize>()
                    .map_err(|err| format!("invalid value for '--page-size': {err}"))?;

                Ok(ExplorerCmd::ReleaseHistory {
                    state,
                    proposer,
                    start_epoch,
                    end_epoch,
                    page,
                    page_size,
                })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub fn handle(cmd: ExplorerCmd) {
    match cmd {
        ExplorerCmd::ReleaseHistory {
            state,
            proposer,
            start_epoch,
            end_epoch,
            page,
            page_size,
        } => {
            let store = GovStore::open(&state);
            let mut releases = match controller::approved_releases(&store) {
                Ok(list) => list,
                Err(err) => {
                    eprintln!("failed to load releases: {err}");
                    return;
                }
            };
            releases.sort_by_key(|r| Reverse(r.activated_epoch));
            let filtered: Vec<_> = releases
                .into_iter()
                .filter(|r| {
                    proposer
                        .as_ref()
                        .map(|p| r.proposer.eq_ignore_ascii_case(p))
                        .unwrap_or(true)
                        && start_epoch.map(|s| r.activated_epoch >= s).unwrap_or(true)
                        && end_epoch.map(|e| r.activated_epoch <= e).unwrap_or(true)
                })
                .collect();
            let size = page_size.max(1);
            let start = page.saturating_mul(size);
            let end = (start + size).min(filtered.len());
            println!("showing releases {}-{} of {}", start, end, filtered.len());
            for release in filtered.into_iter().skip(start).take(size) {
                let last_install = release.install_times.last().cloned().unwrap_or(0);
                println!(
                    "hash={} proposer={} epoch={} installs={} last_install={} threshold={} quorum={}",
                    release.build_hash,
                    release.proposer,
                    release.activated_epoch,
                    release.install_times.len(),
                    last_install,
                    release.signature_threshold,
                    release.signatures.len() as u32 >= release.signature_threshold,
                );
            }
        }
    }
}

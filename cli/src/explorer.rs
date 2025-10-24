use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use explorer::Explorer as ExplorerStore;
use foundation_serialization::json;
use std::cmp::Reverse;
use std::io::{self, Write};
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
    /// Display per-role subsidy and advertising totals for a block
    BlockPayouts {
        db: String,
        hash: Option<String>,
        height: Option<u64>,
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
            .subcommand(
                CommandBuilder::new(
                    CommandId("explorer.block_payouts"),
                    "block-payouts",
                    "Show per-role subsidy and advertising totals for a block",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("db", "db", "Explorer database path").default("explorer.db"),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "hash",
                    "hash",
                    "Block hash to inspect",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "height",
                    "height",
                    "Block height to inspect",
                )))
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
            "block-payouts" => {
                let db = sub_matches
                    .get_string("db")
                    .unwrap_or_else(|| "explorer.db".to_string());
                let hash = sub_matches.get_string("hash");
                let height = match sub_matches.get_string("height") {
                    Some(value) => Some(
                        value
                            .parse::<u64>()
                            .map_err(|err| format!("invalid value for '--height': {err}"))?,
                    ),
                    None => None,
                };
                if hash.is_some() && height.is_some() {
                    return Err("cannot supply both '--hash' and '--height'".into());
                }
                if hash.is_none() && height.is_none() {
                    return Err("must supply '--hash' or '--height'".into());
                }
                Ok(ExplorerCmd::BlockPayouts { db, hash, height })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub fn handle(cmd: ExplorerCmd) {
    let mut stdout = io::stdout();
    if let Err(err) = handle_with_writer(cmd, &mut stdout) {
        eprintln!("{err}");
    }
}

pub fn handle_with_writer(cmd: ExplorerCmd, writer: &mut impl Write) -> Result<(), String> {
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
            let mut releases = controller::approved_releases(&store)
                .map_err(|err| format!("failed to load releases: {err}"))?;
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
            writeln!(
                writer,
                "showing releases {}-{} of {}",
                start,
                end,
                filtered.len()
            )
            .map_err(|err| format!("failed to write output: {err}"))?;
            for release in filtered.into_iter().skip(start).take(size) {
                let last_install = release.install_times.last().cloned().unwrap_or(0);
                writeln!(
                    writer,
                    "hash={} proposer={} epoch={} installs={} last_install={} threshold={} quorum={}",
                    release.build_hash,
                    release.proposer,
                    release.activated_epoch,
                    release.install_times.len(),
                    last_install,
                    release.signature_threshold,
                    release.signatures.len() as u32 >= release.signature_threshold,
                )
                .map_err(|err| format!("failed to write output: {err}"))?;
            }
            Ok(())
        }
        ExplorerCmd::BlockPayouts { db, hash, height } => {
            let store = ExplorerStore::open(&db)
                .map_err(|err| format!("failed to open explorer database {db}: {err}"))?;
            let hash_value = match (hash.as_ref(), height) {
                (Some(hash_value), None) => hash_value.clone(),
                (None, Some(height_value)) => store
                    .block_hash_by_height(height_value)
                    .map_err(|err| {
                        format!("failed to resolve block hash at height {height_value}: {err}")
                    })?
                    .ok_or_else(|| format!("no block found at height {height_value}"))?,
                _ => return Err("must supply exactly one of '--hash' or '--height'".into()),
            };
            let breakdown = store
                .block_payouts(&hash_value)
                .map_err(|err| format!("failed to fetch block payouts for {hash_value}: {err}"))?
                .ok_or_else(|| format!("no block found with hash {hash_value}"))?;
            let payload = json::to_vec_pretty(&breakdown.to_json_value())
                .map_err(|err| format!("failed to serialize payouts: {err}"))?;
            writer
                .write_all(&payload)
                .and_then(|_| writer.write_all(b"\n"))
                .map_err(|err| format!("failed to write output: {err}"))?;
            Ok(())
        }
    }
}

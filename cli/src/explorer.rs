use clap::Subcommand;
use std::cmp::Reverse;
use the_block::governance::{controller, GovStore};

#[derive(Subcommand)]
pub enum ExplorerCmd {
    /// Show release history with optional filters
    ReleaseHistory {
        #[arg(long, default_value = "governance_db")]
        state: String,
        #[arg(long)]
        proposer: Option<String>,
        #[arg(long)]
        start_epoch: Option<u64>,
        #[arg(long)]
        end_epoch: Option<u64>,
        #[arg(long, default_value_t = 0)]
        page: usize,
        #[arg(long, default_value_t = 20)]
        page_size: usize,
    },
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

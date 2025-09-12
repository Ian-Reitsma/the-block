use clap::Subcommand;
use the_block::governance::{GovStore, Proposal, ProposalStatus, Vote, VoteChoice};

#[derive(Subcommand)]
pub enum GovCmd {
    /// List all proposals
    List {
        #[arg(long, default_value = "gov.db")]
        state: String,
    },
    /// Vote on a proposal
    Vote {
        id: u64,
        choice: String,
        #[arg(long, default_value = "gov.db")]
        state: String,
    },
    /// Show proposal status
    Status {
        id: u64,
        #[arg(long, default_value = "gov.db")]
        state: String,
    },
}

pub fn handle(cmd: GovCmd) {
    match cmd {
        GovCmd::List { state } => {
            let store = GovStore::open(state);
            for item in store.proposals().iter() {
                if let Ok((_, raw)) = item {
                    if let Ok(p) = bincode::deserialize::<Proposal>(&raw) {
                        println!("{} {:?}", p.id, p.status);
                    }
                }
            }
        }
        GovCmd::Vote { id, choice, state } => {
            let store = GovStore::open(state);
            let c = match choice.as_str() {
                "yes" => VoteChoice::Yes,
                "no" => VoteChoice::No,
                _ => VoteChoice::Abstain,
            };
            let v = Vote {
                proposal_id: id,
                voter: "cli".into(),
                choice: c,
                weight: 1,
                received_at: 0,
            };
            if store.vote(id, v, 0).is_err() {
                eprintln!("vote failed");
            }
        }
        GovCmd::Status { id, state } => {
            let store = GovStore::open(state);
            let key = bincode::serialize(&id).unwrap();
            if let Ok(Some(raw)) = store.proposals().get(key) {
                if let Ok(p) = bincode::deserialize::<Proposal>(&raw) {
                    println!("{:?}", p.status);
                }
            }
        }
    }
}

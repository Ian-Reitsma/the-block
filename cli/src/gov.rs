use clap::Subcommand;
use the_block::governance::{
    controller, GovStore, Proposal, ProposalStatus, ReleaseBallot, ReleaseVote, Vote, VoteChoice,
};

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
    /// Submit a release vote referencing a signed hash
    ProposeRelease {
        hash: String,
        #[arg(long)]
        signature: Option<String>,
        #[arg(long, default_value = "gov.db")]
        state: String,
        #[arg(long, default_value_t = 32)]
        deadline: u64,
    },
    /// Cast a vote in favour of a release proposal
    ApproveRelease {
        id: u64,
        #[arg(long, default_value = "gov.db")]
        state: String,
        #[arg(long, default_value_t = 1)]
        weight: u64,
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
        GovCmd::ProposeRelease {
            hash,
            signature,
            state,
            deadline,
        } => {
            let store = GovStore::open(state);
            let proposal = ReleaseVote::new(hash, signature, "cli".into(), 0, deadline);
            match controller::submit_release(&store, proposal) {
                Ok(id) => println!("release proposal {id} submitted"),
                Err(e) => eprintln!("submit failed: {e}"),
            }
        }
        GovCmd::ApproveRelease { id, state, weight } => {
            let store = GovStore::open(state);
            let ballot = ReleaseBallot {
                proposal_id: id,
                voter: "cli".into(),
                choice: VoteChoice::Yes,
                weight,
                received_at: 0,
            };
            if let Err(e) = controller::vote_release(&store, ballot) {
                eprintln!("vote failed: {e}");
            }
            if let Ok(status) = controller::tally_release(&store, id, 0) {
                println!("release status: {:?}", status);
            }
        }
    }
}

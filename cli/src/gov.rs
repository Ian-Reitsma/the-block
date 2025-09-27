use clap::Subcommand;
use governance::{
    controller, encode_runtime_backend_policy, encode_storage_engine_policy,
    encode_transport_provider_policy, registry, GovStore, ParamKey, Proposal, ProposalStatus,
    ReleaseAttestation as GovReleaseAttestation, ReleaseBallot, ReleaseVerifier, ReleaseVote, Vote,
    VoteChoice,
};
use the_block::{governance::release::ReleaseAttestation as NodeReleaseAttestation, provenance};

struct CliReleaseVerifier;

impl ReleaseVerifier for CliReleaseVerifier {
    fn configured_signers(&self) -> Vec<String> {
        provenance::release_signer_hexes()
    }

    fn verify(&self, build_hash: &str, signer_hex: &str, signature_hex: &str) -> bool {
        provenance::parse_signer_hex(signer_hex)
            .and_then(|vk| {
                if provenance::verify_release_attestation(build_hash, &vk, signature_hex) {
                    Some(())
                } else {
                    None
                }
            })
            .is_some()
    }
}

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
        #[arg(long = "signature")]
        signatures: Vec<String>,
        #[arg(long = "signer")]
        signers: Vec<String>,
        #[arg(long)]
        threshold: Option<u32>,
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
    /// Download and verify an approved release artifact
    FetchRelease {
        hash: String,
        #[arg(long = "signature")]
        signatures: Vec<String>,
        #[arg(long = "signer")]
        signers: Vec<String>,
        #[arg(long)]
        dest: Option<String>,
        #[arg(long)]
        install: bool,
    },
    /// Parameter management helpers
    Param {
        #[command(subcommand)]
        action: GovParamCmd,
    },
}

#[derive(Subcommand)]
pub enum GovParamCmd {
    /// Submit a governance proposal to update a parameter
    Update {
        /// Parameter key (e.g. `mempool.fee_floor_window`)
        key: String,
        /// Proposed new value
        value: String,
        #[arg(long, default_value = "gov.db")]
        state: String,
        #[arg(long, default_value_t = 0)]
        epoch: u64,
        #[arg(long)]
        vote_deadline: Option<u64>,
        #[arg(long)]
        proposer: Option<String>,
    },
}

fn parse_param_key(name: &str) -> Option<ParamKey> {
    match name {
        "mempool.fee_floor_window" | "FeeFloorWindow" => Some(ParamKey::FeeFloorWindow),
        "mempool.fee_floor_percentile" | "FeeFloorPercentile" => Some(ParamKey::FeeFloorPercentile),
        "runtime.backend" | "RuntimeBackend" => Some(ParamKey::RuntimeBackend),
        "transport.provider" | "TransportProvider" => Some(ParamKey::TransportProvider),
        "storage.engine_policy" | "StorageEnginePolicy" => Some(ParamKey::StorageEnginePolicy),
        _ => None,
    }
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
            mut signatures,
            mut signers,
            threshold,
            state,
            deadline,
        } => {
            let store = GovStore::open(state);
            if !signatures.is_empty() && !signers.is_empty() && signers.len() != signatures.len() {
                eprintln!("--signer count must match --signature count");
                return;
            }
            if signers.is_empty() && !signatures.is_empty() {
                let configured = the_block::provenance::release_signer_hexes();
                if signatures.len() == 1 && configured.len() == 1 {
                    signers = configured;
                } else {
                    eprintln!("--signer must be supplied for each signature");
                    return;
                }
            }
            let attestations: Vec<GovReleaseAttestation> = signatures
                .drain(..)
                .zip(signers.drain(..))
                .map(|(signature, signer)| GovReleaseAttestation { signer, signature })
                .collect();
            let threshold_value = threshold.unwrap_or_default();
            let proposal = ReleaseVote::new(
                hash,
                attestations,
                threshold_value,
                "cli".into(),
                0,
                deadline,
            );
            match controller::submit_release(&store, proposal, Some(&CliReleaseVerifier)) {
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
        GovCmd::FetchRelease {
            hash,
            mut signatures,
            mut signers,
            dest,
            install,
        } => {
            if !signatures.is_empty() && !signers.is_empty() && signatures.len() != signers.len() {
                eprintln!("--signer count must match --signature count");
                return;
            }
            if signers.is_empty() && !signatures.is_empty() {
                let configured = the_block::provenance::release_signer_hexes();
                if signatures.len() == 1 && configured.len() == 1 {
                    signers = configured;
                } else {
                    eprintln!("--signer must be supplied for each signature");
                    return;
                }
            }
            let attestations: Vec<NodeReleaseAttestation> = signatures
                .drain(..)
                .zip(signers.drain(..))
                .map(|(signature, signer)| NodeReleaseAttestation { signer, signature })
                .collect();
            let dest_path = dest.as_deref().map(std::path::Path::new);
            match the_block::update::fetch_release(&hash, &attestations, dest_path) {
                Ok(download) => {
                    println!(
                        "downloaded {} to {}",
                        download.hash,
                        download.path.display()
                    );
                    if install {
                        if let Err(err) = the_block::update::install_release(&download.hash) {
                            eprintln!("install guard failed: {err}");
                        } else {
                            println!("install recorded for {}", download.hash);
                        }
                    }
                }
                Err(err) => eprintln!("fetch failed: {err}"),
            }
        }
        GovCmd::Param { action } => match action {
            GovParamCmd::Update {
                key,
                value,
                state,
                epoch,
                vote_deadline,
                proposer,
            } => {
                let Some(param_key) = parse_param_key(&key) else {
                    eprintln!("unknown parameter key: {key}");
                    return;
                };
                let Some(spec) = registry().iter().find(|s| s.key == param_key) else {
                    eprintln!("parameter not governable: {key}");
                    return;
                };
                let new_value = match param_key {
                    ParamKey::RuntimeBackend => {
                        let entries: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_ascii_lowercase())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        match encode_runtime_backend_policy(entries.iter().map(|s| s.as_str())) {
                            Ok(mask) => mask,
                            Err(err) => {
                                eprintln!("invalid runtime backend policy: {err}");
                                return;
                            }
                        }
                    }
                    ParamKey::TransportProvider => {
                        let entries: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_ascii_lowercase())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        match encode_transport_provider_policy(entries.iter().map(|s| s.as_str())) {
                            Ok(mask) => mask,
                            Err(err) => {
                                eprintln!("invalid transport provider policy: {err}");
                                return;
                            }
                        }
                    }
                    ParamKey::StorageEnginePolicy => {
                        let entries: Vec<String> = value
                            .split(',')
                            .map(|s| s.trim().to_ascii_lowercase())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        match encode_storage_engine_policy(entries.iter().map(|s| s.as_str())) {
                            Ok(mask) => mask,
                            Err(err) => {
                                eprintln!("invalid storage engine policy: {err}");
                                return;
                            }
                        }
                    }
                    _ => match value.parse::<i64>() {
                        Ok(v) => v,
                        Err(err) => {
                            eprintln!("failed to parse value `{value}`: {err}");
                            return;
                        }
                    },
                };

                if new_value < spec.min || new_value > spec.max {
                    eprintln!(
                        "value {} out of bounds (min {} max {})",
                        new_value, spec.min, spec.max
                    );
                    return;
                }
                let store = GovStore::open(&state);
                let proposal = Proposal {
                    id: 0,
                    key: param_key,
                    new_value,
                    min: spec.min,
                    max: spec.max,
                    proposer: proposer.unwrap_or_else(|| "cli".into()),
                    created_epoch: epoch,
                    vote_deadline_epoch: vote_deadline.unwrap_or(epoch),
                    activation_epoch: None,
                    status: ProposalStatus::Open,
                    deps: Vec::new(),
                };
                match controller::submit_proposal(&store, proposal) {
                    Ok(id) => println!("parameter proposal {id} submitted"),
                    Err(e) => eprintln!("submit failed: {e}"),
                }
            }
        },
    }
}

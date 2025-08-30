use super::RpcError;
use crate::governance::{
    GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice,
};
use serde_json::json;

fn parse_key(k: &str) -> Option<ParamKey> {
    match k {
        "SnapshotIntervalSecs" => Some(ParamKey::SnapshotIntervalSecs),
        "ConsumerFeeComfortP90Microunits" => Some(ParamKey::ConsumerFeeComfortP90Microunits),
        "IndustrialAdmissionMinCapacity" => Some(ParamKey::IndustrialAdmissionMinCapacity),
        "CreditsDecayLambdaPerHourPpm" => Some(ParamKey::CreditsDecayLambdaPerHourPpm),
        _ => None,
    }
}

pub fn gov_propose(
    store: &GovStore,
    proposer: String,
    key: &str,
    new_value: i64,
    min: i64,
    max: i64,
    current_epoch: u64,
    vote_deadline: u64,
) -> Result<serde_json::Value, RpcError> {
    let key = parse_key(key).ok_or(RpcError {
        code: -32060,
        message: "bad key",
    })?;
    let p = Proposal {
        id: 0,
        key,
        new_value,
        min,
        max,
        proposer,
        created_epoch: current_epoch,
        vote_deadline_epoch: vote_deadline,
        activation_epoch: None,
        status: ProposalStatus::Open,
    };
    let id = store.submit(p).map_err(|_| RpcError {
        code: -32061,
        message: "submit failed",
    })?;
    Ok(json!({"id": id}))
}

pub fn gov_vote(
    store: &GovStore,
    voter: String,
    proposal_id: u64,
    choice: &str,
    current_epoch: u64,
) -> Result<serde_json::Value, RpcError> {
    let choice = match choice {
        "yes" => VoteChoice::Yes,
        "no" => VoteChoice::No,
        _ => VoteChoice::Abstain,
    };
    let v = Vote {
        proposal_id,
        voter,
        choice,
        weight: 1,
        received_at: current_epoch,
    };
    store
        .vote(proposal_id, v, current_epoch)
        .map_err(|_| RpcError {
            code: -32062,
            message: "vote failed",
        })?;
    Ok(json!({"ok":true}))
}

pub fn gov_list(store: &GovStore) -> Result<serde_json::Value, RpcError> {
    let mut arr = vec![];
    for item in store.proposals().iter() {
        // need access; make proposals() pub
        let (_, raw) = item.map_err(|_| RpcError {
            code: -32063,
            message: "iter",
        })?;
        let p: Proposal = bincode::deserialize(&raw).map_err(|_| RpcError {
            code: -32065,
            message: "decode",
        })?;
        arr.push(p);
    }
    Ok(serde_json::to_value(arr).map_err(|_| RpcError {
        code: -32066,
        message: "json",
    })?)
}

pub fn gov_params(params: &Params, epoch: u64) -> Result<serde_json::Value, RpcError> {
    Ok(json!({
        "epoch": epoch,
        "snapshot_interval_secs": params.snapshot_interval_secs,
        "consumer_fee_comfort_p90_microunits": params.consumer_fee_comfort_p90_microunits,
        "industrial_admission_min_capacity": params.industrial_admission_min_capacity,
        "credits_decay_lambda_per_hour_ppm": params.credits_decay_lambda_per_hour_ppm,
    }))
}

pub fn gov_rollback_last(
    store: &GovStore,
    params: &mut Params,
    rt: &mut Runtime,
    current_epoch: u64,
) -> Result<serde_json::Value, RpcError> {
    store
        .rollback_last(current_epoch, rt, params)
        .map_err(|_| RpcError {
            code: -32064,
            message: "rollback failed",
        })?;
    Ok(json!({"ok":true}))
}

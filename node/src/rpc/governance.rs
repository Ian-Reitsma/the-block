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
        "BetaStorageSubCt" => Some(ParamKey::BetaStorageSubCt),
        "GammaReadSubCt" => Some(ParamKey::GammaReadSubCt),
        "KappaCpuSubCt" => Some(ParamKey::KappaCpuSubCt),
        "LambdaBytesOutSubCt" => Some(ParamKey::LambdaBytesOutSubCt),
        "RentRateCtPerByte" => Some(ParamKey::RentRateCtPerByte),
        "MinerRewardLogisticTarget" => Some(ParamKey::MinerRewardLogisticTarget),
        _ => None,
    }
}

pub fn submit_proposal(
    store: &GovStore,
    proposer: String,
    key: &str,
    new_value: i64,
    min: i64,
    max: i64,
    deps: Vec<u64>,
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
        deps,
    };
    let id = store.submit(p).map_err(|_| RpcError {
        code: -32061,
        message: "submit failed",
    })?;
    Ok(json!({"id": id}))
}

pub fn vote_proposal(
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
    // ensure dependencies activated
    let tree = store.proposals();
    if let Some(raw) = tree
        .get(bincode::serialize(&proposal_id).unwrap())
        .map_err(|_| RpcError {
            code: -32068,
            message: "storage",
        })?
    {
        let prop: Proposal = bincode::deserialize(&raw).map_err(|_| RpcError {
            code: -32069,
            message: "decode",
        })?;
        for dep in &prop.deps {
            if let Some(dr) = tree
                .get(bincode::serialize(dep).unwrap())
                .map_err(|_| RpcError {
                    code: -32068,
                    message: "storage",
                })?
            {
                let dp: Proposal = bincode::deserialize(&dr).map_err(|_| RpcError {
                    code: -32069,
                    message: "decode",
                })?;
                if dp.status != ProposalStatus::Activated {
                    return Err(RpcError {
                        code: -32070,
                        message: "dependency not active",
                    });
                }
            }
        }
    }
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

// Backwards-compatible wrappers
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
    submit_proposal(
        store,
        proposer,
        key,
        new_value,
        min,
        max,
        vec![],
        current_epoch,
        vote_deadline,
    )
}

pub fn gov_vote(
    store: &GovStore,
    voter: String,
    proposal_id: u64,
    choice: &str,
    current_epoch: u64,
) -> Result<serde_json::Value, RpcError> {
    vote_proposal(store, voter, proposal_id, choice, current_epoch)
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
        "beta_storage_sub_ct": params.beta_storage_sub_ct,
        "gamma_read_sub_ct": params.gamma_read_sub_ct,
        "kappa_cpu_sub_ct": params.kappa_cpu_sub_ct,
        "lambda_bytes_out_sub_ct": params.lambda_bytes_out_sub_ct,
        "rent_rate_ct_per_byte": params.rent_rate_ct_per_byte,
        "miner_hysteresis": params.miner_hysteresis,
    }))
}

pub fn inflation_params(params: &Params) -> serde_json::Value {
    json!({
        "beta_storage_sub_ct": params.beta_storage_sub_ct,
        "gamma_read_sub_ct": params.gamma_read_sub_ct,
        "kappa_cpu_sub_ct": params.kappa_cpu_sub_ct,
        "lambda_bytes_out_sub_ct": params.lambda_bytes_out_sub_ct,
        "rent_rate_ct_per_byte": params.rent_rate_ct_per_byte,
    })
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

pub fn gov_rollback(
    store: &GovStore,
    proposal_id: u64,
    params: &mut Params,
    rt: &mut Runtime,
    current_epoch: u64,
) -> Result<serde_json::Value, RpcError> {
    store
        .rollback_proposal(proposal_id, current_epoch, rt, params)
        .map_err(|_| RpcError {
            code: -32067,
            message: "rollback failed",
        })?;
    Ok(json!({"ok":true}))
}

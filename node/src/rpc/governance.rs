use super::RpcError;
use crate::governance::{
    decode_runtime_backend_policy, decode_storage_engine_policy, decode_transport_provider_policy,
    GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice,
};
use foundation_serialization::{binary, Serialize};

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProposalSubmissionResponse {
    pub id: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct OperationStatusResponse {
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReleaseSignersResponse {
    pub signers: Vec<String>,
    pub threshold: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InflationParamsResponse {
    pub beta_storage_sub_ct: i64,
    pub gamma_read_sub_ct: i64,
    pub kappa_cpu_sub_ct: i64,
    pub lambda_bytes_out_sub_ct: i64,
    pub rent_rate_ct_per_byte: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct GovParamsResponse {
    #[serde(flatten)]
    pub params: Params,
    pub epoch: u64,
    pub runtime_backend_policy: Vec<String>,
    pub runtime_backend_mask: i64,
    pub transport_provider_policy: Vec<String>,
    pub transport_provider_mask: i64,
    pub storage_engine_policy: Vec<String>,
    pub storage_engine_mask: i64,
}

fn parse_key(k: &str) -> Option<ParamKey> {
    match k {
        "SnapshotIntervalSecs" => Some(ParamKey::SnapshotIntervalSecs),
        "ConsumerFeeComfortP90Microunits" => Some(ParamKey::ConsumerFeeComfortP90Microunits),
        "IndustrialAdmissionMinCapacity" => Some(ParamKey::IndustrialAdmissionMinCapacity),
        "FeeFloorWindow" => Some(ParamKey::FeeFloorWindow),
        "FeeFloorPercentile" => Some(ParamKey::FeeFloorPercentile),
        "BetaStorageSubCt" => Some(ParamKey::BetaStorageSubCt),
        "GammaReadSubCt" => Some(ParamKey::GammaReadSubCt),
        "KappaCpuSubCt" => Some(ParamKey::KappaCpuSubCt),
        "LambdaBytesOutSubCt" => Some(ParamKey::LambdaBytesOutSubCt),
        "TreasuryPercentCt" => Some(ParamKey::TreasuryPercentCt),
        "RentRateCtPerByte" => Some(ParamKey::RentRateCtPerByte),
        "MinerRewardLogisticTarget" => Some(ParamKey::MinerRewardLogisticTarget),
        "BadgeExpirySecs" => Some(ParamKey::BadgeExpirySecs),
        "JurisdictionRegion" => Some(ParamKey::JurisdictionRegion),
        "KalmanRShort" => Some(ParamKey::KalmanRShort),
        "KalmanRMed" => Some(ParamKey::KalmanRMed),
        "KalmanRLong" => Some(ParamKey::KalmanRLong),
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
) -> Result<ProposalSubmissionResponse, RpcError> {
    let key = parse_key(key).ok_or_else(|| RpcError::new(-32060, "bad key"))?;
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
    let id = store
        .submit(p)
        .map_err(|_| RpcError::new(-32061, "submit failed"))?;
    Ok(ProposalSubmissionResponse { id })
}

pub fn vote_proposal(
    store: &GovStore,
    voter: String,
    proposal_id: u64,
    choice: &str,
    current_epoch: u64,
) -> Result<OperationStatusResponse, RpcError> {
    let choice = match choice {
        "yes" => VoteChoice::Yes,
        "no" => VoteChoice::No,
        _ => VoteChoice::Abstain,
    };
    // ensure dependencies activated
    let tree = store.proposals();
    if let Some(raw) = tree
        .get(binary::encode(&proposal_id).unwrap())
        .map_err(|_| RpcError::new(-32068, "storage"))?
    {
        let prop: Proposal = binary::decode(&raw).map_err(|_| RpcError::new(-32069, "decode"))?;
        for dep in &prop.deps {
            if let Some(dr) = tree
                .get(binary::encode(dep).unwrap())
                .map_err(|_| RpcError::new(-32068, "storage"))?
            {
                let dp: Proposal =
                    binary::decode(&dr).map_err(|_| RpcError::new(-32069, "decode"))?;
                if dp.status != ProposalStatus::Activated {
                    return Err(RpcError::new(-32070, "dependency not active"));
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
        .map_err(|_| RpcError::new(-32062, "vote failed"))?;
    Ok(OperationStatusResponse { ok: true })
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
) -> Result<ProposalSubmissionResponse, RpcError> {
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
) -> Result<OperationStatusResponse, RpcError> {
    vote_proposal(store, voter, proposal_id, choice, current_epoch)
}

pub fn gov_list(store: &GovStore) -> Result<Vec<Proposal>, RpcError> {
    let mut arr = vec![];
    for item in store.proposals().iter() {
        // need access; make proposals() pub
        let (_, raw) = item.map_err(|_| RpcError::new(-32063, "iter"))?;
        let p: Proposal = binary::decode(&raw).map_err(|_| RpcError::new(-32065, "decode"))?;
        arr.push(p);
    }
    Ok(arr)
}

pub fn gov_params(params: &Params, epoch: u64) -> Result<GovParamsResponse, RpcError> {
    Ok(GovParamsResponse {
        params: params.clone(),
        epoch,
        runtime_backend_policy: decode_runtime_backend_policy(params.runtime_backend_policy),
        runtime_backend_mask: params.runtime_backend_policy,
        transport_provider_policy: decode_transport_provider_policy(
            params.transport_provider_policy,
        ),
        transport_provider_mask: params.transport_provider_policy,
        storage_engine_policy: decode_storage_engine_policy(params.storage_engine_policy),
        storage_engine_mask: params.storage_engine_policy,
    })
}

pub fn release_signers(store: &GovStore) -> Result<ReleaseSignersResponse, RpcError> {
    let signers = crate::provenance::release_signer_hexes();
    let threshold = store
        .approved_release_hashes()
        .map_err(|_| RpcError::new(-32080, "release read failed"))?
        .into_iter()
        .max_by_key(|r| r.activated_epoch)
        .map(|r| r.signature_threshold)
        .unwrap_or_else(|| {
            if signers.is_empty() {
                0
            } else {
                signers.len() as u32
            }
        });
    Ok(ReleaseSignersResponse { signers, threshold })
}

pub fn inflation_params(params: &Params) -> InflationParamsResponse {
    InflationParamsResponse {
        beta_storage_sub_ct: params.beta_storage_sub_ct,
        gamma_read_sub_ct: params.gamma_read_sub_ct,
        kappa_cpu_sub_ct: params.kappa_cpu_sub_ct,
        lambda_bytes_out_sub_ct: params.lambda_bytes_out_sub_ct,
        rent_rate_ct_per_byte: params.rent_rate_ct_per_byte,
    }
}

pub fn gov_rollback_last(
    store: &GovStore,
    params: &mut Params,
    rt: &mut Runtime,
    current_epoch: u64,
) -> Result<OperationStatusResponse, RpcError> {
    store
        .rollback_last(current_epoch, rt, params)
        .map_err(|_| RpcError::new(-32064, "rollback failed"))?;
    Ok(OperationStatusResponse { ok: true })
}

pub fn gov_rollback(
    store: &GovStore,
    proposal_id: u64,
    params: &mut Params,
    rt: &mut Runtime,
    current_epoch: u64,
) -> Result<OperationStatusResponse, RpcError> {
    store
        .rollback_proposal(proposal_id, current_epoch, rt, params)
        .map_err(|_| RpcError::new(-32067, "rollback failed"))?;
    Ok(OperationStatusResponse { ok: true })
}

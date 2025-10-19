use crate::governance::GovStore;
use crate::identity::{
    handle_registry::{HandleError, HandleRegistry},
    DidError, DidRecord, DidRegistry,
};
use crate::transaction::TxDidAnchor;
use foundation_serialization::json::Value;
use foundation_serialization::Serialize;

#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct HandleRegistrationResponse {
    pub address: String,
    pub normalized_handle: String,
    pub normalization_accuracy: String,
}

#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct HandleResolutionResponse {
    pub address: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct WhoAmIResponse {
    pub address: String,
    pub handle: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DidAttestationResponse {
    pub signer: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DidRecordResponse {
    pub address: String,
    pub document: String,
    pub hash: String,
    pub nonce: u64,
    pub updated_at: u64,
    pub public_key: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub remote_attestation: Option<DidAttestationResponse>,
}

#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DidResolutionResponse {
    pub address: String,
    pub document: Option<String>,
    pub hash: Option<String>,
    pub nonce: Option<u64>,
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub remote_attestation: Option<DidAttestationResponse>,
}

pub fn register_handle(
    params: &Value,
    reg: &mut HandleRegistry,
) -> Result<HandleRegistrationResponse, HandleError> {
    let handle = params
        .get("handle")
        .and_then(|v| v.as_str())
        .ok_or(HandleError::Reserved)?;
    let pubkey = params
        .get("pubkey")
        .and_then(|v| v.as_str())
        .ok_or(HandleError::BadSig)?;
    let sig = params
        .get("sig")
        .and_then(|v| v.as_str())
        .ok_or(HandleError::BadSig)?;
    #[cfg(feature = "pq-crypto")]
    let pq_pubkey = params.get("pq_pubkey").and_then(|v| v.as_str());
    let nonce = params
        .get("nonce")
        .and_then(|v| v.as_u64())
        .ok_or(HandleError::LowNonce)?;
    let pk_bytes = crypto_suite::hex::decode(pubkey).map_err(|_| HandleError::BadSig)?;
    let sig_bytes = crypto_suite::hex::decode(sig).map_err(|_| HandleError::BadSig)?;
    #[cfg(feature = "pq-crypto")]
    let pq_bytes = pq_pubkey
        .map(|s| crypto_suite::hex::decode(s).ok())
        .flatten();
    #[cfg(feature = "pq-crypto")]
    let outcome = reg.register_handle(handle, &pk_bytes, pq_bytes.as_deref(), &sig_bytes, nonce)?;
    #[cfg(not(feature = "pq-crypto"))]
    let outcome = reg.register_handle(handle, &pk_bytes, &sig_bytes, nonce)?;
    Ok(HandleRegistrationResponse {
        address: outcome.address,
        normalized_handle: outcome.normalized_handle,
        normalization_accuracy: outcome.accuracy.as_str().to_string(),
    })
}

pub fn resolve_handle(params: &Value, reg: &HandleRegistry) -> HandleResolutionResponse {
    let handle = params.get("handle").and_then(|v| v.as_str()).unwrap_or("");
    let addr = reg.resolve_handle(handle);
    HandleResolutionResponse { address: addr }
}

pub fn whoami(params: &Value, reg: &HandleRegistry) -> WhoAmIResponse {
    let addr = params.get("address").and_then(|v| v.as_str()).unwrap_or("");
    let handle = reg.handle_of(addr);
    WhoAmIResponse {
        address: addr.to_string(),
        handle,
    }
}

fn did_record_json(record: DidRecord) -> DidRecordResponse {
    DidRecordResponse {
        address: record.address,
        document: record.document,
        hash: crypto_suite::hex::encode(record.hash),
        nonce: record.nonce,
        updated_at: record.updated_at,
        public_key: crypto_suite::hex::encode(record.public_key),
        remote_attestation: record.remote_attestation.map(|att| DidAttestationResponse {
            signer: att.signer,
            signature: att.signature,
        }),
    }
}

pub fn anchor_did(
    params: &Value,
    reg: &mut DidRegistry,
    gov: &GovStore,
) -> Result<DidRecordResponse, DidError> {
    let tx: TxDidAnchor = foundation_serialization::json::from_value(params.clone())
        .map_err(|_| DidError::InvalidRequest)?;
    let record = reg.anchor(&tx, Some(gov))?;
    Ok(did_record_json(record))
}

pub fn resolve_did(params: &Value, reg: &DidRegistry) -> DidResolutionResponse {
    let address = params.get("address").and_then(|v| v.as_str()).unwrap_or("");
    match reg.resolve(address) {
        Some(record) => {
            let payload = did_record_json(record);
            DidResolutionResponse {
                address: payload.address.clone(),
                document: Some(payload.document),
                hash: Some(payload.hash),
                nonce: Some(payload.nonce),
                updated_at: Some(payload.updated_at),
                public_key: Some(payload.public_key),
                remote_attestation: payload.remote_attestation,
            }
        }
        None => DidResolutionResponse {
            address: address.to_string(),
            document: None,
            hash: None,
            nonce: None,
            updated_at: None,
            public_key: None,
            remote_attestation: None,
        },
    }
}

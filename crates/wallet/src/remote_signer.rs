use crate::{WalletError, WalletSigner};
use ed25519_dalek::{PublicKey, Signature};
use hex;
use ledger::crypto::remote_tag;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

/// Remote signer communicating over HTTP JSON.
pub struct RemoteSigner {
    endpoint: String,
    client: Client,
    pubkey: PublicKey,
    timeout: Duration,
    retries: u8,
}

#[derive(Deserialize)]
struct PubKeyResp {
    pubkey: String,
}

#[derive(Serialize)]
struct SignReq<'a> {
    trace: &'a str,
    msg: String,
}

#[derive(Deserialize)]
struct SignResp {
    sig: String,
}

impl RemoteSigner {
    /// Connect to a signer at `endpoint`, fetching its public key.
    pub fn connect(endpoint: &str) -> Result<Self, WalletError> {
        Self::connect_with(endpoint, Duration::from_secs(5), 3)
    }

    /// Connect with custom timeout and retry parameters.
    pub fn connect_with(
        endpoint: &str,
        timeout: Duration,
        retries: u8,
    ) -> Result<Self, WalletError> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let resp = client
            .get(format!("{endpoint}/pubkey"))
            .send()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let pk: PubKeyResp = resp
            .json()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let bytes = hex::decode(pk.pubkey).map_err(|e| WalletError::Failure(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(WalletError::Failure("invalid pubkey length".into()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let pubkey =
            PublicKey::from_bytes(&arr).map_err(|e| WalletError::Failure(e.to_string()))?;
        Ok(Self {
            endpoint: endpoint.to_string(),
            client,
            pubkey,
            timeout,
            retries,
        })
    }
}

impl WalletSigner for RemoteSigner {
    fn public_key(&self) -> PublicKey {
        self.pubkey
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError> {
        let tagged = remote_tag(msg);
        let msg_hex = hex::encode(tagged);
        let trace_id = Uuid::new_v4();
        let payload = SignReq {
            trace: &trace_id.to_string(),
            msg: msg_hex,
        };
        for attempt in 0..=self.retries {
            info!(%trace_id, attempt, "remote sign request");
            let res = self
                .client
                .post(format!("{}/sign", self.endpoint))
                .json(&payload)
                .timeout(self.timeout)
                .send();
            match res {
                Ok(resp) => match resp.json::<SignResp>() {
                    Ok(r) => {
                        let sig_bytes =
                            hex::decode(r.sig).map_err(|e| WalletError::Failure(e.to_string()))?;
                        if sig_bytes.len() != 64 {
                            return Err(WalletError::Failure("invalid signature length".into()));
                        }
                        let mut arr = [0u8; 64];
                        arr.copy_from_slice(&sig_bytes);
                        return Signature::from_bytes(&arr)
                            .map_err(|e| WalletError::Failure(e.to_string()));
                    }
                    Err(e) => {
                        if attempt == self.retries {
                            return Err(WalletError::Failure(e.to_string()));
                        }
                        warn!(%trace_id, error=%e, "retrying signer parse");
                    }
                },
                Err(e) => {
                    if attempt == self.retries {
                        if e.is_timeout() {
                            return Err(WalletError::Timeout);
                        }
                        return Err(WalletError::Failure(e.to_string()));
                    }
                    warn!(%trace_id, error=%e, "retrying signer request");
                }
            }
        }
        Err(WalletError::Failure("unreachable".into()))
    }
}

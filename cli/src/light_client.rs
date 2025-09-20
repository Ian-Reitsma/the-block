use std::fs;
use std::path::{Path, PathBuf};

use crate::rpc::RpcClient;
use crate::tx::{TxDidAnchor, TxDidAnchorAttestation};
use anyhow::{anyhow, Context, Result};
use clap::{ArgGroup, Args, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use hex;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

const MAX_DID_DOC_BYTES: usize = 64 * 1024;

#[derive(Subcommand, Debug)]
pub enum LightClientCmd {
    /// Show current proof rebate balance
    RebateStatus {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
    /// Interact with the decentralized identifier registry
    Did {
        #[command(subcommand)]
        action: DidCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum DidCmd {
    /// Anchor a DID document on-chain
    Anchor(DidAnchorArgs),
    /// Resolve the latest DID document for an address
    Resolve(DidResolveArgs),
}

#[derive(Args, Debug, Clone)]
#[command(
    group = ArgGroup::new("owner_key")
        .args(["secret", "secret_file"])
        .required(true)
)]
pub struct DidAnchorArgs {
    /// Path to the DID document JSON file
    pub file: PathBuf,
    /// Override the address used for anchoring (defaults to the public key hex)
    #[arg(long)]
    pub address: Option<String>,
    /// Nonce for replay protection
    #[arg(long)]
    pub nonce: u64,
    /// Hex-encoded owner secret key
    #[arg(long)]
    pub secret: Option<String>,
    /// File containing the owner secret key (hex)
    #[arg(long = "secret-file")]
    pub secret_file: Option<PathBuf>,
    /// Optional remote signer material (JSON or raw hex secret)
    #[arg(long)]
    pub remote_signer: Option<PathBuf>,
    /// JSON-RPC endpoint
    #[arg(long, default_value = "http://127.0.0.1:26658")]
    pub rpc: String,
    /// Skip submission and emit the signed payload for offline broadcast
    #[arg(long)]
    pub sign_only: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DidResolveArgs {
    /// Address whose DID should be resolved
    pub address: String,
    /// JSON-RPC endpoint
    #[arg(long, default_value = "http://127.0.0.1:26658")]
    pub rpc: String,
    /// Emit JSON instead of human-readable output
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AnchorKeyMaterial {
    pub address: Option<String>,
    pub nonce: u64,
    pub owner_secret: Vec<u8>,
    pub remote_secret: Option<Vec<u8>>,
    pub remote_signer_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnchorRemoteAttestation {
    pub signer: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AnchorRecord {
    pub address: String,
    pub document: Value,
    pub hash: String,
    pub nonce: u64,
    pub updated_at: u64,
    pub public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_attestation: Option<AnchorRemoteAttestation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedDid {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_attestation: Option<AnchorRemoteAttestation>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnchorRecordWire {
    address: String,
    document: String,
    hash: String,
    nonce: u64,
    updated_at: u64,
    public_key: String,
    #[serde(default)]
    remote_attestation: Option<AnchorRemoteAttestation>,
}

impl AnchorRecordWire {
    fn into_record(self) -> AnchorRecord {
        let doc = serde_json::from_str(&self.document)
            .unwrap_or_else(|_| Value::String(self.document.clone()));
        AnchorRecord {
            address: self.address,
            document: doc,
            hash: self.hash,
            nonce: self.nonce,
            updated_at: self.updated_at,
            public_key: self.public_key,
            remote_attestation: self.remote_attestation,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ResolvedDidWire {
    address: String,
    #[serde(default)]
    document: Option<String>,
    #[serde(default)]
    hash: Option<String>,
    #[serde(default)]
    nonce: Option<u64>,
    #[serde(default)]
    updated_at: Option<u64>,
    #[serde(default)]
    public_key: Option<String>,
    #[serde(default)]
    remote_attestation: Option<AnchorRemoteAttestation>,
}

impl ResolvedDidWire {
    fn into_record(self) -> ResolvedDid {
        let document = self.document.and_then(|doc| {
            serde_json::from_str(&doc)
                .map(Some)
                .unwrap_or_else(|_| Some(Value::String(doc)))
        });
        ResolvedDid {
            address: self.address,
            document,
            hash: self.hash,
            nonce: self.nonce,
            updated_at: self.updated_at,
            public_key: self.public_key,
            remote_attestation: self.remote_attestation,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RpcEnvelope<T> {
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    error: Option<RpcErrorBody>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LightHeader {
    pub height: u64,
    pub hash: String,
    pub difficulty: u64,
}

#[derive(Serialize)]
struct Payload<'a> {
    jsonrpc: &'static str,
    id: u32,
    method: &'static str,
    params: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<&'a str>,
}

pub fn handle(cmd: LightClientCmd) {
    match cmd {
        LightClientCmd::RebateStatus { url } => {
            let client = RpcClient::from_env();
            if let Err(err) = query_rebate_status(&client, &url) {
                eprintln!("{}", err);
            }
        }
        LightClientCmd::Did { action } => match action {
            DidCmd::Anchor(args) => {
                if let Err(err) = run_anchor_command(args) {
                    eprintln!("{}", err);
                }
            }
            DidCmd::Resolve(args) => {
                if let Err(err) = run_resolve_command(args) {
                    eprintln!("{}", err);
                }
            }
        },
    }
}

fn query_rebate_status(client: &RpcClient, url: &str) -> Result<()> {
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "light_client.rebate_status",
        params: serde_json::json!({}),
        auth: None,
    };
    let response = client
        .call(url, &payload)
        .context("rebate status RPC call failed")?;
    let text = response
        .text()
        .context("failed to read rebate status response")?;
    println!("{}", text);
    Ok(())
}

fn run_anchor_command(args: DidAnchorArgs) -> Result<()> {
    let (document, material) = prepare_anchor_inputs(&args)?;
    let tx = build_anchor_transaction(&document, &material)?;
    if args.sign_only {
        let payload = serde_json::to_value(&tx).context("serialize anchor payload")?;
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).context("pretty-print anchor payload")?
        );
        return Ok(());
    }
    let client = RpcClient::from_env();
    let record = submit_anchor(&client, &args.rpc, &tx)?;
    let header = latest_header(&client, &args.rpc)?;
    println!(
        "Anchored DID {} with hash {} at height {}",
        record.address, record.hash, header.height
    );
    println!("Nonce: {}", record.nonce);
    if let Some(att) = &record.remote_attestation {
        println!("Remote signer: {}", att.signer);
    }
    Ok(())
}

fn run_resolve_command(args: DidResolveArgs) -> Result<()> {
    let client = RpcClient::from_env();
    let record = resolve_did_record(&client, &args.rpc, &args.address)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&record).context("serialize resolve output")?
        );
        return Ok(());
    }
    println!("Address: {}", record.address);
    match &record.hash {
        Some(hash) => println!("Hash: {}", hash),
        None => println!("Hash: <none>"),
    }
    match record.nonce {
        Some(nonce) => println!("Nonce: {}", nonce),
        None => println!("Nonce: <none>"),
    }
    match record.updated_at {
        Some(ts) => println!("Updated at: {}", ts),
        None => println!("Updated at: <none>"),
    }
    match &record.document {
        Some(doc) => {
            println!(
                "Document:\n{}",
                serde_json::to_string_pretty(doc).unwrap_or_else(|_| doc.to_string())
            );
        }
        None => println!("Document: <none>"),
    }
    if let Some(att) = &record.remote_attestation {
        println!("Remote signer: {}", att.signer);
    }
    Ok(())
}

fn prepare_anchor_inputs(args: &DidAnchorArgs) -> Result<(Value, AnchorKeyMaterial)> {
    let contents = fs::read_to_string(&args.file)
        .with_context(|| format!("failed to read DID document from {}", args.file.display()))?;
    let document: Value = serde_json::from_str(&contents)
        .with_context(|| format!("DID document {} is not valid JSON", args.file.display()))?;

    let owner_secret = if let Some(secret) = &args.secret {
        decode_secret(secret)
    } else if let Some(path) = &args.secret_file {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read secret key from {}", path.display()))?;
        decode_secret(&text)
    } else {
        Err(anyhow!("missing owner secret key"))
    }?;

    let mut material = AnchorKeyMaterial {
        address: args.address.clone(),
        nonce: args.nonce,
        owner_secret,
        remote_secret: None,
        remote_signer_hex: None,
    };

    if let Some(path) = &args.remote_signer {
        let (secret, signer) = load_remote_signer(path)?;
        material.remote_secret = Some(secret);
        material.remote_signer_hex = signer.map(|s| s.to_lowercase());
    }

    Ok((document, material))
}

fn decode_secret(input: &str) -> Result<Vec<u8>> {
    let trimmed = input.trim();
    let normalized = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = hex::decode(normalized).context("secret key must be hex encoded")?;
    if bytes.len() != 32 && bytes.len() != 64 {
        return Err(anyhow!("secret key must be 32 or 64 bytes"));
    }
    Ok(bytes)
}

fn load_remote_signer(path: &Path) -> Result<(Vec<u8>, Option<String>)> {
    let raw = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read remote signer material from {}",
            path.display()
        )
    })?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("remote signer material is empty"));
    }
    if trimmed.starts_with('{') {
        #[derive(Deserialize)]
        struct RemoteSignerFile {
            secret: String,
            #[serde(default)]
            signer: Option<String>,
        }
        let parsed: RemoteSignerFile = serde_json::from_str(trimmed)
            .context("remote signer file must be JSON with 'secret'")?;
        let secret = decode_secret(&parsed.secret)?;
        Ok((secret, parsed.signer))
    } else {
        let secret = decode_secret(trimmed)?;
        Ok((secret, None))
    }
}

fn key_from_bytes(bytes: &[u8]) -> Result<SigningKey> {
    match bytes.len() {
        32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(bytes);
            Ok(SigningKey::from_bytes(&arr))
        }
        64 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            Ok(SigningKey::from_bytes(&arr))
        }
        _ => Err(anyhow!("ed25519 private key must be 32 or 64 bytes")),
    }
}

pub fn build_anchor_transaction(doc: &Value, material: &AnchorKeyMaterial) -> Result<TxDidAnchor> {
    let canonical = serde_json::to_string(doc).context("canonicalize DID document")?;
    if canonical.as_bytes().len() > MAX_DID_DOC_BYTES {
        return Err(anyhow!("DID document exceeds {} bytes", MAX_DID_DOC_BYTES));
    }
    let owner_key = key_from_bytes(&material.owner_secret)?;
    let owner_public = owner_key.verifying_key().to_bytes();
    let address = material
        .address
        .clone()
        .unwrap_or_else(|| hex::encode(owner_public));

    let mut tx = TxDidAnchor {
        address,
        public_key: owner_public.to_vec(),
        document: canonical,
        nonce: material.nonce,
        signature: Vec::new(),
        remote_attestation: None,
    };
    let owner_sig = owner_key.sign(tx.owner_digest().as_ref());
    tx.signature = owner_sig.to_bytes().to_vec();

    if let Some(remote_secret) = &material.remote_secret {
        let remote_key = key_from_bytes(remote_secret)?;
        let derived = hex::encode(remote_key.verifying_key().to_bytes());
        let signer_hex = material
            .remote_signer_hex
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| derived.clone());
        if signer_hex != derived {
            return Err(anyhow!("remote signer hex does not match provided secret"));
        }
        let att_sig = remote_key.sign(tx.remote_digest().as_ref());
        tx.remote_attestation = Some(TxDidAnchorAttestation {
            signer: signer_hex,
            signature: hex::encode(att_sig.to_bytes()),
        });
    }

    Ok(tx)
}

pub fn submit_anchor(client: &RpcClient, url: &str, tx: &TxDidAnchor) -> Result<AnchorRecord> {
    let params = serde_json::to_value(tx).context("serialize anchor request")?;
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "identity.anchor",
        params,
        auth: None,
    };
    let resp = client
        .call(url, &payload)
        .context("identity.anchor RPC call failed")?
        .json::<RpcEnvelope<Value>>()
        .context("failed to decode identity.anchor response")?;
    if let Some(err) = resp.error {
        return Err(anyhow!(
            "identity.anchor error {} (code {})",
            err.message,
            err.code
        ));
    }
    let result = resp
        .result
        .ok_or_else(|| anyhow!("missing identity.anchor result"))?;
    if let Some(code) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("identity.anchor rejected request: {}", code));
    }
    let wire: AnchorRecordWire =
        serde_json::from_value(result).context("unexpected identity.anchor response format")?;
    Ok(wire.into_record())
}

pub fn latest_header(client: &RpcClient, url: &str) -> Result<LightHeader> {
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "light.latest_header",
        params: Value::Null,
        auth: None,
    };
    let resp = client
        .call(url, &payload)
        .context("light.latest_header RPC call failed")?
        .json::<RpcEnvelope<LightHeader>>()
        .context("failed to decode light.latest_header response")?;
    if let Some(err) = resp.error {
        return Err(anyhow!(
            "light.latest_header error {} (code {})",
            err.message,
            err.code
        ));
    }
    resp.result
        .ok_or_else(|| anyhow!("missing light.latest_header result"))
}

pub fn resolve_did_record(client: &RpcClient, url: &str, address: &str) -> Result<ResolvedDid> {
    let params = serde_json::json!({ "address": address });
    let payload = Payload {
        jsonrpc: "2.0",
        id: 1,
        method: "identity.resolve",
        params,
        auth: None,
    };
    let resp = client
        .call(url, &payload)
        .context("identity.resolve RPC call failed")?
        .json::<RpcEnvelope<Value>>()
        .context("failed to decode identity.resolve response")?;
    if let Some(err) = resp.error {
        return Err(anyhow!(
            "identity.resolve error {} (code {})",
            err.message,
            err.code
        ));
    }
    let result = resp
        .result
        .ok_or_else(|| anyhow!("missing identity.resolve result"))?;
    if let Some(code) = result.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!(
            "identity.resolve returned application error: {}",
            code
        ));
    }
    let wire: ResolvedDidWire =
        serde_json::from_value(result).context("unexpected identity.resolve response format")?;
    Ok(wire.into_record())
}

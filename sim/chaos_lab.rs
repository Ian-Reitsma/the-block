#![forbid(unsafe_code)]

use crypto_suite::hex;
use crypto_suite::signatures::ed25519::{SigningKey, SECRET_KEY_LENGTH};
use foundation_serialization::json::{self, Value};
use foundation_time::UtcDateTime;
use monitoring_build::{sign_attestation, ChaosAttestation};
use sim::Simulation;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn signing_key_from_env() -> Result<SigningKey, Box<dyn Error>> {
    match env::var("TB_CHAOS_SIGNING_KEY") {
        Ok(hex_key) => {
            let key_bytes = hex::decode_array::<SECRET_KEY_LENGTH>(&hex_key)
                .map_err(|_| "TB_CHAOS_SIGNING_KEY must be a valid hex-encoded ed25519 secret")?;
            Ok(SigningKey::from_bytes(&key_bytes))
        }
        Err(_) => {
            use rand::rngs::OsRng;
            eprintln!("[chaos-lab] TB_CHAOS_SIGNING_KEY missing; generating ephemeral signing key");
            let mut rng = OsRng;
            Ok(SigningKey::generate(&mut rng))
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let steps = env::var("TB_CHAOS_STEPS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120);
    let nodes = env::var("TB_CHAOS_NODE_COUNT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(256);
    let dashboard_path = env::var("TB_CHAOS_DASHBOARD").ok();
    let attestation_path =
        env::var("TB_CHAOS_ATTESTATIONS").unwrap_or_else(|_| "chaos_attestations.json".to_string());
    let signing_key = signing_key_from_env()?;

    let mut sim = Simulation::new(nodes);
    if let Some(ref path) = dashboard_path {
        sim.run(steps, path)?;
    } else {
        sim.drive(steps);
    }

    let issued_at = UtcDateTime::now().unix_timestamp().unwrap_or_default() as u64;
    let drafts = sim.chaos_attestation_drafts(issued_at);
    let attestations: Vec<ChaosAttestation> = drafts
        .into_iter()
        .map(|draft| sign_attestation(draft, &signing_key))
        .collect();

    persist_attestations(&attestation_path, &attestations)?;
    eprintln!(
        "[chaos-lab] captured {} attestations for modules: {}",
        attestations.len(),
        format_modules(&attestations)
    );
    eprintln!(
        "[chaos-lab] verifier={}",
        hex::encode(signing_key.verifying_key().to_bytes())
    );
    Ok(())
}

fn persist_attestations(
    path: &str,
    attestations: &[ChaosAttestation],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = File::create(path)?;
    let payload = Value::Array(attestations.iter().map(|att| att.to_value()).collect());
    let data = json::to_vec_pretty(&payload)?;
    file.write_all(&data)?;
    file.flush()?;
    Ok(())
}

fn format_modules(attestations: &[ChaosAttestation]) -> String {
    let mut modules: Vec<&'static str> =
        attestations.iter().map(|att| att.module.as_str()).collect();
    modules.sort();
    modules.dedup();
    modules.join(",")
}

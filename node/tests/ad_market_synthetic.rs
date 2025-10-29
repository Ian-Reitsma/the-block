#![cfg(feature = "integration-tests")]

use std::collections::HashMap;
use std::sync::Arc;

use ad_market::{
    selection_commitment, AttestationSatisfaction, Campaign, CampaignTargeting, Creative,
    ImpressionContext, InMemoryMarketplace, Marketplace, MarketplaceConfig, PacingParameters,
    ReservationKey, SelectionAttestation, SelectionAttestationConfig, SelectionAttestationManager,
    SelectionCommitteeTranscript, SelectionReceipt,
};
use crypto_suite::hashing::blake3;
use foundation_serialization::json::Value;
use the_block::rpc::ad_market as ad_rpc;
use zkp::selection::{
    compute_transcript_digest, selection_circuit_summaries, SelectionProofPublicInputs,
};

const CIRCUIT_ID: &str = "selection_argmax_v1";
const PROOF_LEN: usize = 96;

fn encode_bytes(bytes: &[u8]) -> String {
    let mut encoded = String::from("[");
    for (idx, byte) in bytes.iter().enumerate() {
        if idx > 0 {
            encoded.push(',');
        }
        use std::fmt::Write;
        write!(&mut encoded, "{}", byte).expect("encode byte");
    }
    encoded.push(']');
    encoded
}

fn reservation_key(seed: u64) -> ReservationKey {
    let mut manifest = [0u8; 32];
    manifest.copy_from_slice(blake3::hash(&seed.to_le_bytes()).as_bytes());
    let mut path_hash = [0u8; 32];
    path_hash
        .copy_from_slice(blake3::hash(&[b'p', b'a', b't', b'h', (seed & 0xFF) as u8]).as_bytes());
    let mut discriminator = [0u8; 32];
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&seed.to_le_bytes());
    buf[8..].copy_from_slice(&(seed.wrapping_mul(31)).to_le_bytes());
    discriminator.copy_from_slice(blake3::hash(&buf).as_bytes());
    ReservationKey {
        manifest,
        path_hash,
        discriminator,
    }
}

fn synthesize_snark(receipt: &SelectionReceipt) -> (Vec<u8>, [u8; 32]) {
    let commitment = selection_commitment(receipt).expect("commitment");
    let winner = &receipt.candidates[receipt.winner_index];
    let summary = selection_circuit_summaries()
        .into_iter()
        .find(|summary| summary.circuit_id == CIRCUIT_ID)
        .expect("circuit summary");
    let revision = summary.revision;
    let protocol = summary
        .expected_protocol
        .as_deref()
        .unwrap_or("groth16")
        .to_string();
    let inputs = SelectionProofPublicInputs {
        commitment: commitment.to_vec(),
        winner_index: receipt.winner_index as u16,
        winner_quality_bid_usd_micros: winner.quality_adjusted_bid_usd_micros,
        runner_up_quality_bid_usd_micros: receipt.runner_up_quality_bid_usd_micros,
        resource_floor_usd_micros: receipt.resource_floor_usd_micros,
        clearing_price_usd_micros: receipt.clearing_price_usd_micros,
        candidate_count: receipt.candidates.len() as u16,
    };
    let transcript = compute_transcript_digest(CIRCUIT_ID, &inputs).expect("digest");
    let mut proof_bytes = vec![0xE5; PROOF_LEN];
    proof_bytes[..transcript.len()].copy_from_slice(&transcript);
    let witness_a = blake3::hash(winner.campaign_id.as_bytes());
    let witness_b = blake3::hash(receipt.cohort.domain.as_bytes());
    let witnesses = format!(
        "[{},{}]",
        encode_bytes(witness_a.as_bytes()),
        encode_bytes(witness_b.as_bytes())
    );
    let public_inputs = format!(
        r#"{{"commitment":{},"winner_index":{},"winner_quality_bid_usd_micros":{},"runner_up_quality_bid_usd_micros":{},"resource_floor_usd_micros":{},"clearing_price_usd_micros":{},"candidate_count":{}}}"#,
        encode_bytes(&inputs.commitment),
        inputs.winner_index,
        inputs.winner_quality_bid_usd_micros,
        inputs.runner_up_quality_bid_usd_micros,
        inputs.resource_floor_usd_micros,
        inputs.clearing_price_usd_micros,
        inputs.candidate_count
    );
    let proof_payload = format!(
        r#"{{"version":1,"circuit_revision":{},"public_inputs":{},"proof":{{"protocol":"{}","transcript_digest":{},"bytes":{},"witness_commitments":{}}}}}"#,
        revision,
        public_inputs,
        protocol,
        encode_bytes(&transcript),
        encode_bytes(&proof_bytes),
        witnesses
    );
    (proof_payload.into_bytes(), transcript)
}

fn make_campaign(id: usize, budget: u64) -> Campaign {
    let creative = Creative {
        id: format!("creative-{id}-0"),
        action_rate_ppm: 42_000,
        margin_ppm: 920_000,
        value_per_action_usd_micros: 3_000_000,
        max_cpi_usd_micros: Some(2_800_000),
        lift_ppm: 58_000,
        badges: vec!["badge-k500".into()],
        domains: vec!["example.com".into()],
        metadata: HashMap::new(),
    };
    Campaign {
        id: format!("campaign-{id}"),
        advertiser_account: format!("acct-{id}"),
        budget_usd_micros: budget,
        creatives: vec![creative],
        targeting: CampaignTargeting {
            domains: vec!["example.com".into()],
            badges: vec!["badge-k500".into()],
        },
        metadata: HashMap::new(),
    }
}

fn pacing_config_snapshot(params: &PacingParameters) -> HashMap<&'static str, i64> {
    HashMap::from([
        ("price_eta_p_ppm", params.price_eta_p_ppm as i64),
        ("price_eta_i_ppm", params.price_eta_i_ppm as i64),
        ("price_forgetting_ppm", params.price_forgetting_ppm as i64),
        (
            "target_utilization_ppm",
            params.target_utilization_ppm as i64,
        ),
        ("smoothing_ppm", params.smoothing_ppm as i64),
    ])
}

#[test]
fn synthetic_load_stresses_selection_and_budget_controls() {
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = [CIRCUIT_ID.to_string()].into_iter().collect();
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = false;
    config.budget_broker.step_size = 0.12;
    config.budget_broker.epoch_impressions = 24;
    let market = Arc::new(InMemoryMarketplace::new(config));

    for idx in 0..3 {
        market
            .register_campaign(make_campaign(idx, 18_000_000))
            .expect("register campaign");
    }

    let mut verifier_config = SelectionAttestationConfig::default();
    verifier_config.preferred_circuit_ids = [CIRCUIT_ID.to_string()].into_iter().collect();
    verifier_config.allow_tee_fallback = false;
    verifier_config.require_attestation = true;
    let verifier = SelectionAttestationManager::new(verifier_config);

    for iteration in 0..240u64 {
        let key = reservation_key(iteration);
        let ctx = ImpressionContext {
            domain: "example.com".into(),
            provider: Some("wallet-alpha".into()),
            badges: vec!["badge-k500".into()],
            bytes: 256 + (iteration as u64 % 5) * 32,
            attestations: Vec::new(),
            committee_transcripts: Vec::new(),
            population_estimate: Some(1_200 + iteration as u64 % 23),
        };
        if let Some(outcome) = market.reserve_impression(key, ctx.clone()) {
            let receipt = outcome.selection_receipt;
            let (proof, digest) = synthesize_snark(&receipt);
            let snark_attestation = SelectionAttestation::Snark {
                proof,
                circuit_id: CIRCUIT_ID.into(),
            };
            let committee_transcript = SelectionCommitteeTranscript {
                committee_id: "committee-alpha".into(),
                transcript: receipt.cohort.domain.as_bytes().to_vec(),
                signature: Vec::new(),
                manifest_epoch: None,
                transcript_digest: digest.to_vec(),
            };
            let (attached, satisfaction, proof_metadata, transcripts) = verifier
                .attach_attestation(&receipt, &[snark_attestation], &[committee_transcript]);
            assert!(matches!(satisfaction, AttestationSatisfaction::Satisfied));
            let mut attested = receipt.clone();
            attested.attestation = attached;
            attested.proof_metadata = proof_metadata;
            attested.committee_transcripts = transcripts;
            assert!(verifier.validate_receipt(&attested).is_ok());
            assert!(market.commit(&key).is_some());
        }
    }

    let snapshot = market.budget_snapshot();
    assert_eq!(snapshot.campaigns.len(), 3);
    for campaign in &snapshot.campaigns {
        assert!(campaign.dual_price >= 0.0);
        for cohort in &campaign.cohorts {
            assert!(cohort.kappa >= 0.0);
            assert!(cohort.kappa <= snapshot.config.max_kappa);
        }
    }

    let pacing = market.pacing_parameters();
    let expected_pacing = pacing_config_snapshot(&pacing);

    let handle: ad_market::MarketplaceHandle = market.clone();
    let budget_value = ad_rpc::budget(Some(&handle));
    if let Value::Object(map) = &budget_value {
        assert!(map.contains_key("campaigns"));
        let pacing = map.get("pacing").expect("pacing export");
        if let Value::Object(pacing_obj) = pacing {
            assert_eq!(
                pacing_obj
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("error"),
                "ok"
            );
            assert!(!pacing_obj.contains_key("reason"));
            for (key, expected) in expected_pacing {
                let actual = pacing_obj
                    .get(key)
                    .and_then(|value| value.as_i64())
                    .expect("pacing metric present");
                assert_eq!(actual, expected, "mismatch for {key}");
            }
        } else {
            panic!("pacing payload missing object");
        }
    } else {
        panic!("budget rpc returned unexpected payload");
    }

    let inventory = ad_rpc::inventory(Some(&handle));
    if let Value::Object(map) = &inventory {
        assert!(map.contains_key("budget_broker"));
        assert!(map.contains_key("pacing"));
        assert!(map.contains_key("selection_manifest"));
    } else {
        panic!("inventory payload not object");
    }
}

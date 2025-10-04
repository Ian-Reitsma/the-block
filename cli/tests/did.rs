mod support;

use std::convert::TryInto;

use contract_cli::light_client::{
    build_anchor_transaction, latest_header, resolve_did_record, submit_anchor, AnchorKeyMaterial,
};
use contract_cli::rpc::RpcClient;
use contract_cli::tx::{generate_keypair, TxDidAnchor};
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use hex;
use serde_json::json;
use support::json_rpc::JsonRpcMock;

fn owner_signing_key(bytes: &[u8]) -> SigningKey {
    let arr: [u8; 32] = bytes.try_into().expect("private key length");
    SigningKey::from_bytes(&arr)
}

fn signature_from_vec(sig: &[u8]) -> Signature {
    let arr: [u8; 64] = sig.try_into().expect("signature length");
    Signature::from_bytes(&arr)
}

#[test]
fn build_anchor_transaction_generates_signatures() {
    let document = json!({
        "id": "did:tb:test",
        "service": [{"id": "#resolver", "type": "Resolver", "serviceEndpoint": "https://example.com"}]
    });
    let (owner_secret, owner_public) = generate_keypair();
    let (remote_secret, remote_public) = generate_keypair();

    let material = AnchorKeyMaterial {
        address: None,
        nonce: 7,
        owner_secret: owner_secret.clone(),
        remote_secret: Some(remote_secret.clone()),
        remote_signer_hex: None,
    };

    let tx = build_anchor_transaction(&document, &material).expect("build anchor");
    let owner_key = owner_signing_key(&owner_secret);
    let owner_vk = owner_key.verifying_key();
    assert_eq!(tx.address, hex::encode(&owner_public));
    assert_eq!(tx.public_key, owner_public.clone());
    let parsed_document: serde_json::Value =
        serde_json::from_str(&tx.document).expect("canonical doc");
    assert_eq!(parsed_document, document);
    let sig = signature_from_vec(&tx.signature);
    owner_vk
        .verify(tx.owner_digest().as_ref(), &sig)
        .expect("owner signature verifies");

    let att = tx
        .remote_attestation
        .as_ref()
        .expect("remote attestation present");
    let remote_vk = VerifyingKey::from_bytes(&remote_public.clone().try_into().unwrap()).unwrap();
    assert_eq!(att.signer, hex::encode(&remote_public));
    let remote_sig_bytes = hex::decode(&att.signature).expect("decode remote sig");
    let remote_sig = signature_from_vec(&remote_sig_bytes);
    remote_vk
        .verify(tx.remote_digest().as_ref(), &remote_sig)
        .expect("remote attestation signature verifies");
}

#[test]
fn build_anchor_transaction_rejects_large_documents() {
    let oversized = json!({ "blob": "a".repeat(65_537) });
    let (owner_secret, _) = generate_keypair();
    let material = AnchorKeyMaterial {
        address: None,
        nonce: 1,
        owner_secret,
        remote_secret: None,
        remote_signer_hex: None,
    };
    let err = build_anchor_transaction(&oversized, &material).expect_err("reject large doc");
    assert!(err.to_string().contains("exceeds"));
}

fn anchor_responses(tx: &TxDidAnchor, updated_at: u64) -> String {
    let doc_hash = hex::encode(tx.document_hash());
    json!({
        "jsonrpc": "2.0",
        "result": {
            "address": tx.address,
            "document": tx.document,
            "hash": doc_hash,
            "nonce": tx.nonce,
            "updated_at": updated_at,
            "public_key": hex::encode(&tx.public_key),
            "remote_attestation": tx.remote_attestation.as_ref()
        },
        "id": 1
    })
    .to_string()
}

#[test]
fn anchor_submission_and_resolve_flow() {
    let document = json!({
        "id": "did:tb:flow",
        "controller": ["did:tb:owner"],
        "service": [{"id": "#agent", "type": "AgentService", "serviceEndpoint": "https://agent"}]
    });
    let (owner_secret, owner_public) = generate_keypair();
    let material = AnchorKeyMaterial {
        address: None,
        nonce: 11,
        owner_secret: owner_secret.clone(),
        remote_secret: None,
        remote_signer_hex: None,
    };
    let tx = build_anchor_transaction(&document, &material).expect("build anchor");
    let responses = vec![
        anchor_responses(&tx, 123),
        json!({
            "jsonrpc": "2.0",
            "result": {"height": 42, "hash": "beef", "difficulty": 1},
            "id": 1
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "result": {
                "address": tx.address,
                "document": tx.document,
                "hash": hex::encode(tx.document_hash()),
                "nonce": tx.nonce,
                "updated_at": 123,
                "public_key": hex::encode(&owner_public),
                "remote_attestation": serde_json::Value::Null
            },
            "id": 1
        })
        .to_string(),
    ];
    let server = JsonRpcMock::start(responses);

    let client = RpcClient::from_env();
    let record = submit_anchor(&client, server.url(), &tx).expect("anchor RPC succeeds");
    assert_eq!(record.address, tx.address);
    assert_eq!(record.nonce, tx.nonce);
    assert_eq!(record.hash, hex::encode(tx.document_hash()));
    assert_eq!(record.document, document);

    let header = latest_header(&client, server.url()).expect("latest header");
    assert_eq!(header.height, 42);

    let resolved = resolve_did_record(&client, server.url(), &tx.address).expect("resolve");
    assert_eq!(resolved.address, tx.address);
    assert_eq!(resolved.nonce, Some(tx.nonce));
    assert_eq!(resolved.hash, Some(hex::encode(tx.document_hash())));
    assert_eq!(resolved.document, Some(document));

    let bodies = server.captured();
    assert!(bodies[0].contains("\"method\":\"identity.anchor\""));
    assert!(bodies[1].contains("\"method\":\"light.latest_header\""));
    assert!(bodies[2].contains("\"method\":\"identity.resolve\""));
}

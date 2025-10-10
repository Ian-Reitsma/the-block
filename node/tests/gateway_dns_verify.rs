#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::{ed25519::SigningKey, Signer};
use sys::tempfile::tempdir;
use the_block::gateway::dns::{
    clear_verify_cache, dns_lookup, gateway_policy, publish_record, set_allow_external,
    set_txt_resolver,
};

fn setup(domain: &str) -> (sys::tempfile::TempDir, String, SigningKey) {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    clear_verify_cache();
    set_allow_external(false);
    set_txt_resolver(|_| vec![]);
    let txt = foundation_serialization::json::to_string_value(
        &foundation_serialization::json!({"gw_policy": {}}),
    );
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let pk = sk.verifying_key();
    let mut msg = Vec::new();
    msg.extend(domain.as_bytes());
    msg.extend(txt.as_bytes());
    let sig = sk.sign(&msg);
    let params = foundation_serialization::json!({
        "domain":domain,
        "txt":txt,
        "pubkey":crypto_suite::hex::encode(pk.to_bytes()),
        "sig":crypto_suite::hex::encode(sig.to_bytes()),
    });
    let _ = publish_record(&params);
    (dir, crypto_suite::hex::encode(pk.to_bytes()), sk)
}

#[testkit::tb_serial]
fn block_tld_trusted() {
    let (_dir, _pk_hex, _sk) = setup("good.block");
    let l = dns_lookup(&foundation_serialization::json!({"domain":"good.block"}));
    assert!(l["verified"].as_bool().unwrap());
    let p = gateway_policy(&foundation_serialization::json!({"domain":"good.block"}));
    assert!(p["record"].is_string());
}

#[testkit::tb_serial]
fn external_domain_verified() {
    let (_dir, pk_hex, _sk) = setup("example.com");
    set_allow_external(true);
    let pk_clone = pk_hex.clone();
    set_txt_resolver(move |_| vec![pk_clone.clone()]);
    let l = dns_lookup(&foundation_serialization::json!({"domain":"example.com"}));
    assert!(l["verified"].as_bool().unwrap());
    let p = gateway_policy(&foundation_serialization::json!({"domain":"example.com"}));
    assert!(p["record"].is_string());
}

#[testkit::tb_serial]
fn external_domain_rejected() {
    let (_dir, _pk_hex, _sk) = setup("bad.com");
    set_allow_external(true);
    set_txt_resolver(|_| vec!["other".into()]);
    let l = dns_lookup(&foundation_serialization::json!({"domain":"bad.com"}));
    assert!(!l["verified"].as_bool().unwrap());
    let p = gateway_policy(&foundation_serialization::json!({"domain":"bad.com"}));
    assert!(p["record"].is_null());
}

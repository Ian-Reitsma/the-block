#![cfg(feature = "integration-tests")]
use the_block::gateway::dns::{
    clear_verify_cache, set_allow_external, set_disable_verify, set_txt_resolver, verify_txt,
};

#[test]
fn verifies_token() {
    set_allow_external(true);
    clear_verify_cache();
    set_txt_resolver(|_| vec!["tb-verification=node1".to_string()]);
    assert!(verify_txt("example.com", "node1"));
    assert!(!verify_txt("example.com", "other"));
}

#[test]
fn disable_verification_allows() {
    set_disable_verify(true);
    assert!(verify_txt("example.org", "whatever"));
    set_disable_verify(false);
}

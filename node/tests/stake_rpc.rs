#![cfg(feature = "integration-tests")]
use the_block::rpc::pos;
use wallet::Wallet;

#[test]
fn bond_and_unbond_via_rpc() {
    let seed = [9u8; 32];
    let w = Wallet::from_seed(&seed);
    let role = "gateway";
    let amount = 7u64;
    let sig = w.sign_stake(role, amount, false).unwrap();
    let pk_hex = w.public_key_hex();
    let sig_hex = crypto_suite::hex::encode(sig.to_bytes());
    let params = foundation_serialization::json!({
        "id": pk_hex.clone(),
        "role": role,
        "amount": amount,
        "sig": sig_hex.clone(),
        "signers": [{"pk": pk_hex.clone(), "sig": sig_hex.clone()}],
        "threshold": 1,
    });
    let res = pos::bond(&params).expect("bond");
    assert_eq!(res["stake"].as_u64().unwrap(), amount);
    let sig_u = w.sign_stake(role, amount, true).unwrap();
    let sig_u_hex = crypto_suite::hex::encode(sig_u.to_bytes());
    let params_u = foundation_serialization::json!({
        "id": pk_hex.clone(),
        "role": role,
        "amount": amount,
        "sig": sig_u_hex.clone(),
        "signers": [{"pk": pk_hex.clone(), "sig": sig_u_hex}],
        "threshold": 1,
    });
    let res_u = pos::unbond(&params_u).expect("unbond");
    assert_eq!(res_u["stake"].as_u64().unwrap(), 0);

    let params_role = foundation_serialization::json!({"id": pk_hex, "role": role});
    let res_role = pos::role(&params_role).expect("role");
    assert_eq!(res_role["stake"].as_u64().unwrap(), 0);
}

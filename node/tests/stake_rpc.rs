use the_block::rpc::pos;
use serde_json::json;
use wallet::Wallet;

#[test]
fn bond_and_unbond_via_rpc() {
    let seed = [9u8;32];
    let w = Wallet::from_seed(&seed);
    let role = "gateway";
    let amount = 7u64;
    let sig = w.sign_stake(role, amount, false).unwrap();
    let params = json!({
        "id": w.public_key_hex(),
        "role": role,
        "amount": amount,
        "sig": hex::encode(sig.to_bytes()),
    });
    let res = pos::bond(&params).expect("bond");
    assert_eq!(res["stake"].as_u64().unwrap(), amount);
    let sig_u = w.sign_stake(role, amount, true).unwrap();
    let params_u = json!({
        "id": w.public_key_hex(),
        "role": role,
        "amount": amount,
        "sig": hex::encode(sig_u.to_bytes()),
    });
    let res_u = pos::unbond(&params_u).expect("unbond");
    assert_eq!(res_u["stake"].as_u64().unwrap(), 0);
}

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
    let mut params = foundation_serialization::json::Map::new();
    params.insert(
        "id".to_string(),
        foundation_serialization::json::Value::String(pk_hex.clone()),
    );
    params.insert(
        "role".to_string(),
        foundation_serialization::json::Value::String(role.to_string()),
    );
    params.insert(
        "amount".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(amount),
        ),
    );
    params.insert(
        "sig".to_string(),
        foundation_serialization::json::Value::String(sig_hex.clone()),
    );
    let mut signer = foundation_serialization::json::Map::new();
    signer.insert(
        "pk".to_string(),
        foundation_serialization::json::Value::String(pk_hex.clone()),
    );
    signer.insert(
        "sig".to_string(),
        foundation_serialization::json::Value::String(sig_hex.clone()),
    );
    params.insert(
        "signers".to_string(),
        foundation_serialization::json::Value::Array(vec![
            foundation_serialization::json::Value::Object(signer),
        ]),
    );
    params.insert(
        "threshold".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(1),
        ),
    );
    let res = pos::bond(&foundation_serialization::json::Value::Object(params)).expect("bond");
    assert_eq!(res.stake, amount);
    let sig_u = w.sign_stake(role, amount, true).unwrap();
    let sig_u_hex = crypto_suite::hex::encode(sig_u.to_bytes());
    let mut params_u = foundation_serialization::json::Map::new();
    params_u.insert(
        "id".to_string(),
        foundation_serialization::json::Value::String(pk_hex.clone()),
    );
    params_u.insert(
        "role".to_string(),
        foundation_serialization::json::Value::String(role.to_string()),
    );
    params_u.insert(
        "amount".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(amount),
        ),
    );
    params_u.insert(
        "sig".to_string(),
        foundation_serialization::json::Value::String(sig_u_hex.clone()),
    );
    let mut signer_u = foundation_serialization::json::Map::new();
    signer_u.insert(
        "pk".to_string(),
        foundation_serialization::json::Value::String(pk_hex.clone()),
    );
    signer_u.insert(
        "sig".to_string(),
        foundation_serialization::json::Value::String(sig_u_hex),
    );
    params_u.insert(
        "signers".to_string(),
        foundation_serialization::json::Value::Array(vec![
            foundation_serialization::json::Value::Object(signer_u),
        ]),
    );
    params_u.insert(
        "threshold".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(1),
        ),
    );
    let res_u =
        pos::unbond(&foundation_serialization::json::Value::Object(params_u)).expect("unbond");
    assert_eq!(res_u.stake, 0);

    let mut params_role = foundation_serialization::json::Map::new();
    params_role.insert(
        "id".to_string(),
        foundation_serialization::json::Value::String(pk_hex),
    );
    params_role.insert(
        "role".to_string(),
        foundation_serialization::json::Value::String(role.to_string()),
    );
    let res_role =
        pos::role(&foundation_serialization::json::Value::Object(params_role)).expect("role");
    assert_eq!(res_role.stake, 0);
}

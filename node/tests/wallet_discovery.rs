use foundation_serialization::json::{self, Value};
use the_block::wallet_discovery::discovery_result_json;

#[test]
fn discover_signers_json_structure_is_stable() {
    let signers = vec![
        "http://127.0.0.1:7878".to_string(),
        "https://remote-signer.local".to_string(),
    ];
    let result = discovery_result_json(750, &signers);
    let expected: Value = json::from_str(
        r#"{
            "timeout_ms": 750,
            "signers": ["http://127.0.0.1:7878", "https://remote-signer.local"]
        }"#,
    )
    .expect("parse expected json");
    assert_eq!(result, expected);
}

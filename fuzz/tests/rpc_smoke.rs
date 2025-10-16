#[path = "../rpc/mod.rs"]
mod rpc;

use foundation_serialization::json::Value;
use the_block::Blockchain;

#[test]
fn consensus_difficulty_through_harness() {
    let request = foundation_rpc::Request::new("consensus.difficulty", Value::Null);
    let response = rpc::run_request(request);
    match response {
        foundation_rpc::Response::Result { result, .. } => {
            let difficulty = result
                .get("difficulty")
                .and_then(Value::as_u64)
                .expect("difficulty field");
            assert_eq!(difficulty, Blockchain::default().difficulty);
        }
        foundation_rpc::Response::Error { error, .. } => {
            panic!("unexpected error: {error:?}");
        }
    }
}

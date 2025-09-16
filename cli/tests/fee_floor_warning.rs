use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;

use contract_cli::wallet::{build_tx_default_locale, BuildTxStatus};
use the_block::rpc::client::RpcClient;
use the_block::transaction::FeeLane;
use tiny_http::{Header, Response, Server};

fn start_mock_server(
    responses: Vec<String>,
) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let handle = thread::spawn(move || {
        let total = responses.len();
        let mut responses = VecDeque::from(responses);
        let mut incoming = server.incoming_requests();
        for _ in 0..total {
            let mut request = match incoming.next() {
                Some(req) => req,
                None => break,
            };
            let mut body = String::new();
            request
                .as_reader()
                .read_to_string(&mut body)
                .expect("read request body");
            captured_clone.lock().unwrap().push(body);
            let response_body = responses
                .pop_front()
                .unwrap_or_else(|| "{\"status\":\"ok\"}".to_string());
            let mut response = Response::from_string(response_body);
            response.add_header(Header::from_bytes(b"Content-Type", b"application/json").unwrap());
            request.respond(response).expect("send response");
        }
    });
    (addr, captured, handle)
}

#[test]
fn auto_bump_emits_warning_event() {
    let stats = "{\"jsonrpc\":\"2.0\",\"result\":{\"fee_floor\":10,\"size\":0,\"age_p50\":0,\"age_p95\":0,\"fee_p50\":0,\"fee_p90\":0}}".to_string();
    let ack = "{\"status\":\"ok\"}".to_string();
    let (addr, captured, handle) = start_mock_server(vec![stats.clone(), ack.clone()]);
    let client = RpcClient::from_env();
    let report = build_tx_default_locale(
        &client,
        &addr,
        FeeLane::Consumer,
        "alice",
        "bob",
        100,
        2,
        100,
        0,
        &[],
        true,
        false,
        false,
    )
    .expect("build tx");
    handle.join().expect("server thread");
    assert_eq!(report.status, BuildTxStatus::Ready);
    assert!(report.auto_bumped);
    assert_eq!(report.effective_fee, 10);
    let bodies = captured.lock().unwrap();
    assert!(bodies[0].contains("\"method\":\"mempool.stats\""));
    assert!(bodies[1].contains("\"method\":\"mempool.qos_event\""));
    assert!(bodies[1].contains("\"event\":\"warning\""));
}

#[test]
fn force_records_override_metric() {
    let stats = "{\"jsonrpc\":\"2.0\",\"result\":{\"fee_floor\":50,\"size\":0,\"age_p50\":0,\"age_p95\":0,\"fee_p50\":0,\"fee_p90\":0}}".to_string();
    let ack = "{\"status\":\"ok\"}".to_string();
    let (addr, captured, handle) = start_mock_server(vec![stats, ack]);
    let client = RpcClient::from_env();
    let report = build_tx_default_locale(
        &client,
        &addr,
        FeeLane::Consumer,
        "carol",
        "dave",
        200,
        5,
        100,
        1,
        &[],
        false,
        true,
        false,
    )
    .expect("build tx");
    handle.join().expect("server thread");
    assert_eq!(report.status, BuildTxStatus::Ready);
    assert!(report.forced);
    assert_eq!(report.effective_fee, 5);
    let bodies = captured.lock().unwrap();
    assert!(bodies[0].contains("\"method\":\"mempool.stats\""));
    assert!(bodies[1].contains("\"event\":\"override\""));
}

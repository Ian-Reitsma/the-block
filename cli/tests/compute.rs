use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;

use contract_cli::compute::{handle_with_writer, ComputeCmd};
use tiny_http::{Header, Response, Server};

fn start_mock_server(
    responses: Vec<String>,
) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let handle = thread::spawn(move || {
        let mut responses = VecDeque::from(responses);
        for mut request in server.incoming_requests() {
            let mut body = String::new();
            request
                .as_reader()
                .read_to_string(&mut body)
                .expect("read request");
            captured_clone.lock().unwrap().push(body);
            let response_body = responses
                .pop_front()
                .unwrap_or_else(|| "{\"status\":\"ok\"}".to_string());
            let mut response = Response::from_string(response_body);
            response.add_header(Header::from_bytes(b"Content-Type", b"application/json").unwrap());
            request.respond(response).expect("send response");
            if responses.is_empty() {
                break;
            }
        }
    });
    (addr, captured, handle)
}

#[test]
fn stats_includes_provider_balances() {
    let stats = "{\"jsonrpc\":\"2.0\",\"result\":{\"industrial_backlog\":3,\"industrial_utilization\":75,\"industrial_units_total\":9,\"industrial_price_per_unit\":21}}".to_string();
    let balances = "{\"jsonrpc\":\"2.0\",\"result\":{\"providers\":[{\"provider\":\"alice\",\"ct\":42,\"industrial\":7}]}}".to_string();
    let (addr, captured, handle) = start_mock_server(vec![stats, balances]);
    let mut output = Vec::new();
    handle_with_writer(
        ComputeCmd::Stats {
            url: addr.clone(),
            accelerator: None,
        },
        &mut output,
    )
    .expect("stats command");
    handle.join().expect("server thread");
    let bodies = captured.lock().unwrap();
    assert!(bodies[0].contains("\"method\":\"compute_market.stats\""));
    assert!(bodies[1].contains("\"method\":\"compute_market.provider_balances\""));
    let printed = String::from_utf8(output).expect("stdout");
    assert!(printed.contains("provider: alice"));
    assert!(printed.contains("ct: 42"));
    assert!(printed.contains("it: 7"));
}

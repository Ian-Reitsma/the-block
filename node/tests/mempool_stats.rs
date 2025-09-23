#![cfg(feature = "integration-tests")]
use serial_test::serial;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tempfile::tempdir;
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    generate_keypair,
    rpc::{
        client::{RpcClient, WalletQosError, WalletQosEvent},
        run_rpc_server,
    },
    sign_tx, Blockchain, RawTxPayload,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::timeout::expect_timeout;

mod util;

async fn rpc(addr: &str, body: &str) -> serde_json::Value {
    let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    expect_timeout(stream.write_all(req.as_bytes()))
        .await
        .unwrap();
    let mut resp = Vec::new();
    expect_timeout(stream.read_to_end(&mut resp)).await.unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    serde_json::from_str(&resp[body_idx + 4..]).unwrap()
}

#[tokio::test]
#[serial]
async fn mempool_stats_rpc() {
    let dir = tempdir().unwrap();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let expected_floor;
    {
        let mut guard = bc.lock().unwrap();
        guard.add_account("alice".into(), 1000, 0).unwrap();
        guard.add_account("bob".into(), 0, 0).unwrap();
        let (sk, _) = generate_keypair();
        for i in 0..2 {
            let payload = RawTxPayload {
                from_: "alice".into(),
                to: "bob".into(),
                amount_consumer: 1,
                amount_industrial: 0,
                fee: (i + 1) * 10,
                pct_ct: 100,
                nonce: i + 1,
                memo: Vec::new(),
            };
            let tx = sign_tx(sk.to_vec(), payload).unwrap();
            guard.submit_transaction(tx).unwrap();
        }
        expected_floor = guard.mempool_stats(the_block::FeeLane::Consumer).fee_floor;
    }
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = the_block::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        Default::default(),
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();
    let val = rpc(
        &addr,
        r#"{"method":"mempool.stats","params":{"lane":"consumer"}}"#,
    )
    .await;
    assert_eq!(val["result"]["size"].as_u64().unwrap(), 2);
    assert_eq!(val["result"]["fee_p90"].as_u64().unwrap(), 20);
    assert_eq!(val["result"]["fee_floor"].as_u64().unwrap(), expected_floor);
    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
async fn mempool_qos_event_public_rpc() {
    use std::net::TcpListener;
    use std::thread;

    let dir = tempdir().unwrap();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = the_block::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        Default::default(),
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();
    let url = format!("http://{}", addr);
    let client = RpcClient::from_env();
    let event = WalletQosEvent {
        event: "warning",
        lane: "consumer",
        fee: 25,
        floor: 10,
    };

    let client_ack = client.clone();
    let url_ack = url.clone();
    let ack = the_block::spawn_blocking(move || {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "mempool.qos_event",
            "params": {
                "event": event.event,
                "lane": event.lane,
                "fee": event.fee,
                "floor": event.floor,
            }
        });
        client_ack
            .call(&url_ack, &payload)
            .expect("send QoS event")
            .json::<serde_json::Value>()
            .expect("parse QoS response")
    })
    .await
    .unwrap();
    assert_eq!(ack["result"]["status"], "ok");
    assert!(ack.get("error").is_none());

    let client_send = client.clone();
    let url_send = url.clone();
    the_block::spawn_blocking(move || client_send.record_wallet_qos_event(&url_send, event))
        .await
        .unwrap()
        .expect("wallet telemetry call should succeed when ack is ok");

    // Spin up a stub server that responds with a non-ok status to ensure the
    // client surfaces malformed acknowledgements.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stub_addr = listener.local_addr().unwrap();
    let stub_handle = thread::spawn(move || {
        use std::io::{Read, Write};

        let (mut stream, _) = listener.accept().unwrap();
        consume_http_request(&mut stream).unwrap();
        let body = r#"{"jsonrpc":"2.0","result":{"status":"fail"},"id":1}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let stub_url = format!("http://{}", stub_addr);
    let error = the_block::spawn_blocking(move || {
        RpcClient::from_env().record_wallet_qos_event(&stub_url, event)
    })
    .await
    .unwrap()
    .expect_err("wallet telemetry client must reject non-ok acknowledgements");

    match error {
        WalletQosError::InvalidStatus(status) => assert_eq!(status, "fail"),
        other => panic!("unexpected error variant: {other:?}"),
    }

    stub_handle.join().unwrap();

    handle.abort();
    let _ = handle.await;
}

fn consume_http_request(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    use std::io::Read;

    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];

    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            let content_len = parse_content_length(&buf[..pos]);
            let mut remaining = content_len.saturating_sub(buf.len() - pos);
            while remaining > 0 {
                let n = stream.read(&mut tmp)?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                remaining = remaining.saturating_sub(n);
            }
            break;
        }
    }

    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn parse_content_length(headers: &[u8]) -> usize {
    let text = String::from_utf8_lossy(headers);
    for line in text.lines() {
        let mut parts = line.splitn(2, ':');
        if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
            if name.trim().eq_ignore_ascii_case("content-length") {
                if let Ok(len) = value.trim().parse::<usize>() {
                    return len;
                }
            }
        }
    }
    0
}

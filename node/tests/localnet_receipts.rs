use std::sync::{atomic::AtomicBool, Arc, Mutex};
use ed25519_dalek::{Signer, SigningKey};
use std::convert::TryInto;
use serial_test::serial;
use serde_json::Value;
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    localnet::AssistReceipt,
    rpc::run_rpc_server,
    Blockchain, generate_keypair,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::{temp::temp_dir, timeout::expect_timeout};

mod util;

async fn rpc(addr: &str, body: &str) -> Value {
    let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
        body.len(), body
    );
    expect_timeout(stream.write_all(req.as_bytes())).await.unwrap();
    let mut resp = Vec::new();
    expect_timeout(stream.read_to_end(&mut resp)).await.unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    let body = &resp[body_idx + 4..];
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
#[serial]
async fn localnet_receipt_dedups_and_accrues() {
    let dir = temp_dir("localnet_receipts");
    std::env::set_var(
        "TB_LOCALNET_DB_PATH",
        dir.path().join("receipts_db").to_str().unwrap(),
    );
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0);
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let rpc_cfg = RpcConfig::default();
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        rpc_cfg,
        tx,
    ));
    let addr = expect_timeout(rx).await.unwrap();

    let (sk_bytes, _) = generate_keypair();
    let sk_arr: [u8; 32] = sk_bytes.try_into().unwrap();
    let sk = SigningKey::from_bytes(&sk_arr);
    let rssi: i8 = -30;
    let rtt: u32 = 10;
    let mut msg = Vec::new();
    msg.extend(b"alice");
    msg.extend(b"us-west");
    msg.push(rssi as u8);
    msg.extend(&rtt.to_le_bytes());
    let sig = sk.sign(&msg);
    let receipt = AssistReceipt {
        provider: "alice".into(),
        region: "us-west".into(),
        pubkey: sk.verifying_key().to_bytes().to_vec(),
        sig: sig.to_bytes().to_vec(),
        rssi,
        rtt_ms: rtt,
    };
    let hex_receipt = hex::encode(bincode::serialize(&receipt).unwrap());
    let body = format!(
        r#"{{"method":"localnet.submit_receipt","params":{{"receipt":"{}"}}}}"#,
        hex_receipt
    );

    let val = expect_timeout(rpc(&addr, &body)).await;
    assert_eq!(val["result"]["status"], "ok");
    assert_eq!(Settlement::balance("alice"), 1);

    let val2 = expect_timeout(rpc(&addr, &body)).await;
    assert_eq!(val2["result"]["status"], "ignored");
    assert_eq!(Settlement::balance("alice"), 1);

    Settlement::shutdown();
    handle.abort();
}

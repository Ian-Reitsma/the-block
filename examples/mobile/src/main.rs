use light_client::{sync_background, upload_compressed_logs, Header, LightClient, SyncOptions};
use wallet::remote_signer::RemoteSigner;
use reqwest::blocking::Client;
use std::fs::File;
use std::io::Read;
use std::thread;
use tiny_http::{Response, Server};
use tokio::runtime::Runtime;

fn main() {
    // simulate syncing headers from a file on disk
    let mut file = File::open("../light_headers.json").expect("header file");
    let mut data = Vec::new();
    file.read_to_end(&mut data).unwrap();
    let headers: Vec<Header> = serde_json::from_slice(&data).unwrap();
    let mut iter = headers.into_iter();
    let genesis = iter.next().unwrap();
    let mut client = LightClient::new(genesis.clone());
    // simulate partial sync then delta background sync
    if let Some(h) = iter.next() {
        client.verify_and_append(h).expect("header");
    }
    let remaining: Vec<Header> = iter.collect();
    let fetch = move |start: u64, _batch: usize| {
        let remaining = remaining.clone();
        async move {
            remaining
                .into_iter()
                .filter(|h| h.height >= start)
                .collect::<Vec<_>>()
        }
    };
    let opts = SyncOptions {
        wifi_only: true,
        require_charging: false,
        min_battery: 0.1,
        ..SyncOptions::default()
    };
    let rt = Runtime::new().expect("runtime");
    let outcome = rt
        .block_on(async { sync_background(&mut client, opts, fetch).await })
        .expect("sync background");
    println!("synced {} headers", client.chain.len());
    if let Some(reason) = outcome.gating {
        println!("sync gated due to {:?}", reason);
    }
    let bundle = upload_compressed_logs(b"demo log line", Some(&outcome.status));
    println!("compressed log size {} bytes", bundle.payload.len());

    // start a minimal remote signer service
    let server = Server::http("127.0.0.1:0").expect("server");
    let addr = format!("http://{}", server.server_addr());
    thread::spawn(move || {
        let wallet = wallet::Wallet::generate();
        let pk_hex = hex::encode(wallet.public_key().to_bytes());
        for request in server.incoming_requests() {
            match request.url() {
                "/pubkey" => {
                    let _ = request.respond(Response::from_string(format!("{{\"pubkey\":\"{}\"}}", pk_hex)));
                }
                "/sign" => {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let msg_hex = v["msg"].as_str().unwrap();
                    let msg = hex::decode(msg_hex).unwrap();
                    let sig = wallet.sign(&msg).unwrap();
                    let resp = Response::from_string(format!("{{\"sig\":\"{}\"}}", hex::encode(sig.to_bytes())));
                    let _ = request.respond(resp);
                }
                _ => {
                    let _ = request.respond(Response::empty(404));
                }
            }
        }
    });

    // client side signer usage
    let signer = RemoteSigner::connect(&addr).expect("connect");
    let message = b"mobile-wallet-demo";
    let sig = signer.sign(message).expect("sign");
    println!("signature: {}", hex::encode(sig.to_bytes()));

    notify_tx("demo-tx");
    show_trust_lines();
    show_compute_earnings();
    show_gov_votes();
}

fn notify_tx(id: &str) {
    let client = Client::new();
    let _ = client
        .post("http://localhost:8080/push")
        .json(&serde_json::json!({"tx": id}))
        .send();
    println!("push notification: tx {id}");
}

fn show_trust_lines() {
    println!("[ui] trust lines stub");
}

fn show_compute_earnings() {
    println!("[ui] compute earnings stub");
}

fn show_gov_votes() {
    println!("[ui] governance votes stub");
}

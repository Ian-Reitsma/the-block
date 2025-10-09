use httpd::{BlockingClient, HttpError, Method, Response, Router, ServerConfig, StatusCode};
use light_client::{sync_background, upload_compressed_logs, Header, LightClient, SyncOptions};
use runtime::net::TcpListener;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use wallet::{remote_signer::RemoteSigner, Wallet, WalletSigner};
use foundation_serialization::json;

fn main() {
    // simulate syncing headers from a file on disk
    let mut file = File::open("../light_headers.json").expect("header file");
    let mut data = Vec::new();
    file.read_to_end(&mut data).unwrap();
    let headers: Vec<Header> = json::from_slice(&data).unwrap();
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
    let outcome = runtime::block_on(async { sync_background(&mut client, opts, fetch).await })
        .expect("sync background");
    println!("synced {} headers", client.chain.len());
    if let Some(reason) = outcome.gating {
        println!("sync gated due to {:?}", reason);
    }
    let bundle = upload_compressed_logs(b"demo log line", Some(&outcome.status));
    println!("compressed log size {} bytes", bundle.payload.len());

    // start a minimal remote signer service
    let (addr, _handle) = runtime::block_on(async {
        #[derive(Clone)]
        struct SignerState {
            wallet: Arc<Wallet>,
            pk_hex: String,
        }

        #[derive(Deserialize)]
        struct SignRequest {
            msg: String,
        }

        #[derive(Serialize)]
        struct SignResponse {
            sig: String,
        }

        #[derive(Serialize)]
        struct PubKeyResponse {
            pubkey: String,
        }

        let signer_wallet = Wallet::generate();
        let pk_hex = signer_wallet.public_key_hex();
        let state = SignerState {
            wallet: Arc::new(signer_wallet),
            pk_hex,
        };

        let router = Router::new(state.clone())
            .route(Method::Get, "/pubkey", |req| async move {
                let state = req.state().clone();
                Response::new(StatusCode::OK).json(&PubKeyResponse {
                    pubkey: state.pk_hex.clone(),
                })
            })
            .route(Method::Post, "/sign", |req| async move {
                let state = req.state().clone();
                let payload: SignRequest = req.json()?;
                let msg = hex::decode(payload.msg)
                    .map_err(|err| HttpError::Handler(format!("invalid hex payload: {err}")))?;
                let sig = state
                    .wallet
                    .sign(&msg)
                    .map_err(|err| HttpError::Handler(err.to_string()))?;
                Response::new(StatusCode::OK).json(&SignResponse {
                    sig: hex::encode(sig.to_bytes()),
                })
            });

        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind signer listener");
        let addr = listener.local_addr().expect("listener address");
        let handle = runtime::spawn(async move {
            httpd::serve(listener, router, ServerConfig::default()).await
        });
        (format!("http://{}", addr), handle)
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
    let client = BlockingClient::default();
    let _ = client
        .request(Method::Post, "http://localhost:8080/push")
        .and_then(|builder| builder.json(&foundation_serialization::json!({ "tx": id })))
        .and_then(|builder| builder.send());
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

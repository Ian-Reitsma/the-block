use httpd::{HttpError, Method, Response, Router, ServerConfig, StatusCode};
use runtime::net::TcpListener;
use foundation_serialization::{Deserialize, Serialize};
use std::sync::Arc;
use wallet::{remote_signer::RemoteSigner, Wallet, WalletSigner};
use ledger::crypto::remote_tag;
use crypto_suite::signatures::Verifier;

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

fn main() {
    let (endpoint, server) = runtime::block_on(async {
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

    let signer = RemoteSigner::connect(&endpoint).expect("connect");
    let message = b"demo";
    let sig = signer.sign(message).expect("sign");
    signer
        .public_key()
        .verify(&remote_tag(message), &sig)
        .unwrap();
    println!("signature: {}", hex::encode(sig.to_bytes()));

    server.abort();
}

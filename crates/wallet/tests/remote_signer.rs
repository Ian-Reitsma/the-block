use ed25519_dalek::Verifier;
use ledger::crypto::remote_tag;
use std::thread;
use tiny_http::{Response, Server};
use wallet::{remote_signer::RemoteSigner, Wallet, WalletSigner};

fn spawn_mock_signer() -> (String, thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let wallet = Wallet::generate();
    let pk_hex = hex::encode(wallet.public_key().to_bytes());
    let handle = thread::spawn(move || {
        for mut request in server.incoming_requests() {
            match request.url() {
                "/pubkey" => {
                    let resp = Response::from_string(format!("{{\"pubkey\":\"{}\"}}", pk_hex));
                    let _ = request.respond(resp);
                }
                "/sign" => {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let msg_hex = v["msg"].as_str().unwrap();
                    let msg = hex::decode(msg_hex).unwrap();
                    let sig = wallet.sign(&msg).unwrap();
                    let resp = Response::from_string(format!(
                        "{{\"sig\":\"{}\"}}",
                        hex::encode(sig.to_bytes())
                    ));
                    let _ = request.respond(resp);
                    break;
                }
                _ => {
                    let _ = request.respond(Response::empty(404));
                }
            }
        }
    });
    (addr, handle)
}

#[test]
fn remote_signer_roundtrip() {
    let (url, handle) = spawn_mock_signer();
    let signer = RemoteSigner::connect(&url).expect("connect");
    let msg = b"hello";
    let sig = signer.sign(msg).expect("sign");
    signer.public_key().verify(&remote_tag(msg), &sig).unwrap();
    handle.join().unwrap();
}

#[test]
#[ignore]
fn external_signer_manual() {
    if let Ok(url) = std::env::var("REMOTE_SIGNER_URL") {
        let signer = RemoteSigner::connect(&url).unwrap();
        let sig = signer.sign(b"ping").unwrap();
        assert_eq!(sig.to_bytes().len(), 64);
    }
}

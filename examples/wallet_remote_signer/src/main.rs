use std::io::Read;
use std::thread;
use tiny_http::{Response, Server};
use wallet::{remote_signer::RemoteSigner, Wallet, WalletSigner};
use ledger::crypto::remote_tag;
use crypto_suite::signatures::Verifier;

fn main() {
    // Start a minimal signer service in the background.
    let server = Server::http("127.0.0.1:0").expect("server");
    let addr = format!("http://{}", server.server_addr());
    thread::spawn(move || {
        let wallet = Wallet::generate();
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

    // Client side: connect to the signer and request a signature.
    let signer = RemoteSigner::connect(&addr).expect("connect");
    let message = b"demo";
    let sig = signer.sign(message).expect("sign");
    signer
        .public_key()
        .verify(&remote_tag(message), &sig)
        .unwrap();
    println!("signature: {}", hex::encode(sig.to_bytes()));
}

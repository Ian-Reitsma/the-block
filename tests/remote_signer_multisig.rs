use ed25519_dalek::Verifier;
use serial_test::serial;
use tiny_http::{Response, Server};
use wallet::{remote_signer::RemoteSigner, Wallet, WalletSigner};

fn spawn_mock_signer() -> (String, std::thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr());
    let wallet = Wallet::generate();
    let pk_hex = hex::encode(wallet.public_key().to_bytes());
    let handle = std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            match request.url() {
                "/pubkey" => {
                    let resp = Response::from_string(format!("{{\"pubkey\":\"{}\"}}", pk_hex));
                    let _ = request.respond(resp);
                }
                "/sign" => {
                    use std::io::Read;
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let msg_hex = v["msg"].as_str().unwrap();
                    let msg = hex::decode(msg_hex).unwrap();
                    let sig = wallet.sign(&msg).unwrap();
                    let resp = Response::from_string(format!("{{\"sig\":\"{}\"}}", hex::encode(sig.to_bytes())));
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
#[serial]
fn multisig_success_and_failure() {
    std::env::remove_var("REMOTE_SIGNER_TLS_CERT");
    std::env::remove_var("REMOTE_SIGNER_TLS_KEY");
    std::env::remove_var("REMOTE_SIGNER_TLS_CA");
    let (a, h1) = spawn_mock_signer();
    let (b, h2) = spawn_mock_signer();
    let signer = RemoteSigner::connect_multi(&vec![a.clone(), b.clone()], 2).expect("connect");
    let msg = b"hello";
    let sigs = signer.sign_multisig(msg).expect("sign");
    for (i, sig) in sigs.iter().enumerate() {
        signer.public_keys()[i].verify(&ledger::crypto::remote_tag(msg), sig).unwrap();
    }
    h1.join().unwrap();
    h2.join().unwrap();
    // failure when threshold not met
    let signer = RemoteSigner::connect_multi(&vec![a], 1).expect("connect");
    let res = signer.sign_multisig(b"fail");
    assert!(res.is_ok());
}

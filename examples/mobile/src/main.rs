use light_client::{Header, LightClient};
use wallet::{CreditNotifier, Wallet};
use std::fs::File;
use std::io::Read;

fn main() {
    // simulate syncing headers from a file on disk
    let mut file = File::open("../light_headers.json").expect("header file");
    let mut data = Vec::new();
    file.read_to_end(&mut data).unwrap();
    let headers: Vec<Header> = serde_json::from_slice(&data).unwrap();
    let mut iter = headers.into_iter();
    let genesis = iter.next().unwrap();
    let mut client = LightClient::new(genesis);
    for h in iter {
        client.verify_and_append(h).expect("header");
    }
    println!("synced {} headers; credits={}", client.chain.len(), client.credits);

    // basic wallet operation
    let wallet = Wallet::generate();
    let message = b"mobile-wallet-demo";
    let sig = wallet.sign(message).expect("sign");
    println!("signature: {}", hex::encode(sig.to_bytes()));

    // register push endpoint and trigger a balance notification
    let mut notifier = CreditNotifier::default();
    notifier.register_webhook("https://example.com/push");
    let _ = notifier.notify_balance_change("demo", client.credits as u64);
}

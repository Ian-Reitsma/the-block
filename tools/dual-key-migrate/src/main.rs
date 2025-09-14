use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};

#[derive(Deserialize)]
struct LegacyAccount {
    address: String,
    ed25519_pub: String,
}

#[derive(Serialize)]
struct DualAccount {
    address: String,
    ed25519_pub: String,
    dilithium_pub: String,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: dual-key-migrate <in.json> <out.json>");
        std::process::exit(1);
    }
    let mut data = String::new();
    File::open(&args[1])
        .expect("open input")
        .read_to_string(&mut data)
        .expect("read");
    let accounts: Vec<LegacyAccount> = serde_json::from_str(&data).expect("parse");
    let mut out_accounts = Vec::new();
    for acc in accounts {
        let (pk, _sk) = crypto::dilithium::keypair();
        out_accounts.push(DualAccount {
            address: acc.address,
            ed25519_pub: acc.ed25519_pub,
            dilithium_pub: hex::encode(pk),
        });
    }
    let json = serde_json::to_string_pretty(&out_accounts).expect("ser");
    File::create(&args[2])
        .expect("out")
        .write_all(json.as_bytes())
        .expect("write");
}

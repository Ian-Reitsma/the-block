use crypto_suite::hashing::blake3;
use std::env;
use std::fs;

fn main() {
    let mut args = env::args().skip(1);
    let path = args.next().expect("path");
    let expected = args.next().expect("expected hash");
    let bytes = fs::read(&path).expect("read file");
    let actual = blake3::hash(&bytes).to_hex().to_string();
    if actual == expected {
        println!("ok");
    } else {
        eprintln!("mismatch: {actual}");
        std::process::exit(1);
    }
}

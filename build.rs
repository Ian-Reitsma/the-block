use regex::Regex;
use std::fs;
use std::path::Path;

fn calculate_genesis_hash() -> String {
    let mut hasher = blake3::Hasher::new();
    let index = 0u64;
    let prev = "0".repeat(64);
    let nonce = 0u64;
    let difficulty = 8u64;
    let coin_c = 0u64;
    let coin_i = 0u64;
    let fee_checksum = "0".repeat(64);
    hasher.update(&index.to_le_bytes());
    hasher.update(prev.as_bytes());
    hasher.update(&nonce.to_le_bytes());
    hasher.update(&difficulty.to_le_bytes());
    hasher.update(&coin_c.to_le_bytes());
    hasher.update(&coin_i.to_le_bytes());
    hasher.update(fee_checksum.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn main() {
    let contents = fs::read_to_string(Path::new("src/constants.rs")).expect("read constants.rs");
    let re = Regex::new(r#"GENESIS_HASH: &str = \"([0-9a-f]+)\""#).unwrap();
    let const_hash = re.captures(&contents).expect("GENESIS_HASH not found")[1].to_string();
    let calc = calculate_genesis_hash();
    assert_eq!(calc, const_hash, "GENESIS_HASH mismatch");
}

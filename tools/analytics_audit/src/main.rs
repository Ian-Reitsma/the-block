use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{binary, Deserialize};
use std::{
    env,
    fs::File,
    io::{BufReader, Read},
};

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct ReadAck {
    manifest: [u8; 32],
    path_hash: [u8; 32],
    bytes: u64,
    ts: u64,
    client_hash: [u8; 32],
    pk: [u8; 32],
    sig: Vec<u8>,
}

fn hash_ack(a: &ReadAck) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(&a.manifest);
    h.update(&a.path_hash);
    h.update(&a.bytes.to_le_bytes());
    h.update(&a.ts.to_le_bytes());
    h.update(&a.client_hash);
    h.update(&a.pk);
    h.update(&a.sig);
    h.finalize().into()
}

fn merkle_root(mut leaves: Vec<[u8; 32]>) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    while leaves.len() > 1 {
        let mut next = Vec::with_capacity((leaves.len() + 1) / 2);
        for pair in leaves.chunks(2) {
            let mut h = Hasher::new();
            h.update(&pair[0]);
            if pair.len() == 2 {
                h.update(&pair[1]);
            } else {
                h.update(&pair[0]);
            }
            next.push(h.finalize().into());
        }
        leaves = next;
    }
    leaves[0]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .expect("usage: analytics_audit <binary-file>");
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    let acks: Vec<ReadAck> = binary::decode(&buffer)?;
    let leaves: Vec<[u8; 32]> = acks.iter().map(hash_ack).collect();
    let root = merkle_root(leaves);
    let total: u64 = acks.iter().map(|a| a.bytes).sum();
    println!(
        "root:{} total_bytes:{} count:{}",
        crypto_suite::hex::encode(root),
        total,
        acks.len()
    );
    Ok(())
}

use blake3::Hasher;
use serde::{Deserialize, Serialize};

/// Header from an external chain.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Header {
    pub chain_id: String,
    pub height: u64,
    #[serde(with = "hex_array")]
    pub merkle_root: [u8; 32],
    #[serde(with = "hex_array")]
    pub signature: [u8; 32],
}

/// Merkle proof referencing a deposit leaf.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Proof {
    #[serde(with = "hex_array")]
    pub leaf: [u8; 32],
    #[serde(with = "hex_array_vec")]
    pub path: Vec<[u8; 32]>,
}

/// Hashes the header fields for signature comparison and replay protection.
pub fn header_hash(header: &Header) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(header.chain_id.as_bytes());
    h.update(&header.height.to_le_bytes());
    h.update(&header.merkle_root);
    *h.finalize().as_bytes()
}

/// Verifies the header signature and Merkle path.
pub fn verify(header: &Header, proof: &Proof) -> bool {
    if header_hash(header) != header.signature {
        return false;
    }
    let mut acc = proof.leaf;
    for sibling in &proof.path {
        let mut h = Hasher::new();
        h.update(&acc);
        h.update(sibling);
        acc = *h.finalize().as_bytes();
    }
    acc == header.merkle_root
}

mod hex_array {
    use hex::FromHex;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = <[u8; 32]>::from_hex(&s).map_err(serde::de::Error::custom)?;
        Ok(bytes)
    }
}

mod hex_array_vec {
    use hex::FromHex;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(vec: &Vec<[u8; 32]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let strings: Vec<String> = vec.iter().map(|b| hex::encode(b)).collect();
        strings.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<[u8; 32]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let strings: Vec<String> = Vec::deserialize(deserializer)?;
        let mut out = Vec::with_capacity(strings.len());
        for s in strings {
            let bytes = <[u8; 32]>::from_hex(&s).map_err(serde::de::Error::custom)?;
            out.push(bytes);
        }
        Ok(out)
    }
}

use blake3::hash;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A minimal block header for light client verification.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Header {
    pub index: u64,
    pub previous_hash: String,
    pub timestamp_millis: u64,
    pub difficulty: u32,
    pub nonce: u64,
    pub hash: String,
}

impl Header {
    /// Compute the hash of the header fields.
    pub fn compute_hash(&self) -> String {
        let data = format!(
            "{}{}{}{}{}",
            self.index, self.previous_hash, self.timestamp_millis, self.difficulty, self.nonce
        );
        hash(data.as_bytes()).to_hex().to_string()
    }
}

#[derive(Error, Debug)]
pub enum LightClientError {
    #[error("invalid previous hash")]
    InvalidPrevHash,
    #[error("invalid difficulty")]
    InvalidDifficulty,
    #[error("hash mismatch")]
    HashMismatch,
}

/// Simple credit-aware light client.
pub struct LightClient {
    pub chain: Vec<Header>,
    pub credits: u64,
}

impl LightClient {
    /// Initialize the client with a genesis header.
    pub fn new(genesis: Header) -> Self {
        Self { chain: vec![genesis], credits: 1 }
    }

    fn check_difficulty(hash_hex: &str, difficulty: u32) -> bool {
        hash_hex.starts_with(&"0".repeat(difficulty as usize))
    }

    /// Verify and append a header, accruing one credit per block.
    pub fn verify_and_append(&mut self, header: Header) -> Result<(), LightClientError> {
        let prev = self.chain.last().expect("genesis exists");
        if header.previous_hash != prev.hash {
            return Err(LightClientError::InvalidPrevHash);
        }
        if header.compute_hash() != header.hash {
            return Err(LightClientError::HashMismatch);
        }
        if !Self::check_difficulty(&header.hash, header.difficulty) {
            return Err(LightClientError::InvalidDifficulty);
        }
        self.chain.push(header);
        self.credits += 1;
        Ok(())
    }
}

/// Verify a chain of headers encoded as JSON. Exposed for FFI users.
#[no_mangle]
pub extern "C" fn light_client_verify_chain(ptr: *const u8, len: usize) -> bool {
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    let headers: Vec<Header> = match serde_json::from_slice(data) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let mut iter = headers.into_iter();
    let Some(genesis) = iter.next() else { return false };
    let mut client = LightClient::new(genesis);
    for h in iter {
        if client.verify_and_append(h).is_err() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chain() -> Vec<Header> {
        // precomputed chain with difficulty 1
        vec![
            Header {
                index: 0,
                previous_hash: "0".into(),
                timestamp_millis: 0,
                difficulty: 1,
                nonce: 0,
                hash: "09772d00da3679874db0d44e03c6d6536866d3660ccd9bad7f15bf385977e7aa".into(),
            },
            Header {
                index: 1,
                previous_hash: "09772d00da3679874db0d44e03c6d6536866d3660ccd9bad7f15bf385977e7aa".into(),
                timestamp_millis: 1,
                difficulty: 1,
                nonce: 16,
                hash: "0003b8fa7fd5b68b736220a6b4ac054ea2fa74506ec31aecf34e39448555d100".into(),
            },
        ]
    }

    #[test]
    fn chain_verification_and_credits() {
        let mut iter = sample_chain().into_iter();
        let genesis = iter.next().unwrap();
        let mut client = LightClient::new(genesis);
        for h in iter {
            client.verify_and_append(h).unwrap();
        }
        assert_eq!(client.chain.len(), 2);
        assert_eq!(client.credits, 2);
    }

    #[test]
    fn ffi_verification() {
        let chain = sample_chain();
        let json = serde_json::to_vec(&chain).unwrap();
        assert!(light_client_verify_chain(json.as_ptr(), json.len()));
    }
}

use core::fmt;

pub const OUTPUT_LEN: usize = 32;
const STATE_SIZE: usize = 25;
const RATE: usize = 136;
const DOMAIN: u8 = 0x06;

#[derive(Clone)]
pub struct Sha3_256 {
    state: [u64; STATE_SIZE],
    buffer: [u8; RATE],
    buffer_len: usize,
}

impl Sha3_256 {
    pub fn new() -> Self {
        Self {
            state: [0u64; STATE_SIZE],
            buffer: [0u8; RATE],
            buffer_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        if self.buffer_len > 0 {
            let take = RATE - self.buffer_len;
            let chunk = take.min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + chunk].copy_from_slice(&data[..chunk]);
            self.buffer_len += chunk;
            data = &data[chunk..];
            if self.buffer_len == RATE {
                absorb_block(&mut self.state, &self.buffer);
                self.buffer_len = 0;
            }
        }
        while data.len() >= RATE {
            let (block, rest) = data.split_at(RATE);
            absorb_block(&mut self.state, block);
            data = rest;
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    pub fn finalize(mut self) -> Sha3Digest {
        for b in self.buffer[self.buffer_len..RATE].iter_mut() {
            *b = 0;
        }
        self.buffer[self.buffer_len] ^= DOMAIN;
        self.buffer[RATE - 1] ^= 0x80;
        absorb_block(&mut self.state, &self.buffer);
        let mut out = [0u8; OUTPUT_LEN];
        squeeze(&mut self.state, &mut out);
        Sha3Digest(out)
    }
}

impl Default for Sha3_256 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Sha3Digest([u8; OUTPUT_LEN]);

impl Sha3Digest {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl From<Sha3Digest> for [u8; OUTPUT_LEN] {
    fn from(value: Sha3Digest) -> Self {
        value.0
    }
}

impl fmt::Debug for Sha3Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Sha3Digest")
            .field(&crate::hex::encode(self.0))
            .finish()
    }
}

pub fn hash(data: &[u8]) -> [u8; OUTPUT_LEN] {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    hasher.finalize().into()
}

pub fn keyed_hash(key: &[u8; OUTPUT_LEN], data: &[u8]) -> [u8; OUTPUT_LEN] {
    let mut hasher = Sha3_256::new();
    hasher.update(key);
    hasher.update(data);
    hasher.finalize().into()
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; OUTPUT_LEN] {
    let mut hasher = Sha3_256::new();
    hasher.update(context.as_bytes());
    hasher.update(material);
    hasher.finalize().into()
}

fn absorb_block(state: &mut [u64; STATE_SIZE], block: &[u8]) {
    debug_assert_eq!(block.len(), RATE);
    for (lane, chunk) in block.chunks_exact(8).enumerate() {
        let value = u64::from_le_bytes(chunk.try_into().expect("lane"));
        state[lane] ^= value;
    }
    keccak_f1600(state);
}

fn squeeze(state: &mut [u64; STATE_SIZE], out: &mut [u8]) {
    let mut offset = 0;
    loop {
        for lane in 0..(RATE / 8) {
            let bytes = state[lane].to_le_bytes();
            let remaining = out.len() - offset;
            let take = remaining.min(8);
            out[offset..offset + take].copy_from_slice(&bytes[..take]);
            offset += take;
            if offset == out.len() {
                return;
            }
        }
        keccak_f1600(state);
    }
}

#[allow(clippy::needless_range_loop)]
fn keccak_f1600(state: &mut [u64; STATE_SIZE]) {
    const ROTC: [u32; 24] = [
        1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44,
    ];
    const PILN: [usize; 24] = [
        10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1,
    ];
    const RC: [u64; 24] = [
        0x0000_0000_0000_0001,
        0x0000_0000_0000_8082,
        0x8000_0000_0000_808a,
        0x8000_0000_8000_8000,
        0x0000_0000_0000_808b,
        0x0000_0000_8000_0001,
        0x8000_0000_8000_8081,
        0x8000_0000_0000_8009,
        0x0000_0000_0000_008a,
        0x0000_0000_0000_0088,
        0x0000_0000_8000_8009,
        0x0000_0000_8000_000a,
        0x0000_0000_8000_808b,
        0x8000_0000_0000_008b,
        0x8000_0000_0000_8089,
        0x8000_0000_0000_8003,
        0x8000_0000_0000_8002,
        0x8000_0000_0000_0080,
        0x0000_0000_0000_800a,
        0x8000_0000_8000_000a,
        0x8000_0000_8000_8081,
        0x8000_0000_0000_8080,
        0x0000_0000_8000_0001,
        0x8000_0000_8000_8008,
    ];

    for round in 0..24 {
        let mut c = [0u64; 5];
        for i in 0..5 {
            c[i] = state[i] ^ state[i + 5] ^ state[i + 10] ^ state[i + 15] ^ state[i + 20];
        }

        let mut d = [0u64; 5];
        for i in 0..5 {
            d[i] = c[(i + 4) % 5] ^ c[(i + 1) % 5].rotate_left(1);
        }

        for i in 0..25 {
            state[i] ^= d[i % 5];
        }

        let mut current = state[1];
        for i in 0..24 {
            let j = PILN[i];
            let temp = state[j];
            state[j] = current.rotate_left(ROTC[i]);
            current = temp;
        }

        for j in (0..25).step_by(5) {
            let row = [
                state[j],
                state[j + 1],
                state[j + 2],
                state[j + 3],
                state[j + 4],
            ];
            for i in 0..5 {
                state[j + i] = row[i] ^ ((!row[(i + 1) % 5]) & row[(i + 2) % 5]);
            }
        }

        state[0] ^= RC[round];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha3_vectors() {
        let cases: &[(&[u8], &str)] = &[
            (
                &b""[..],
                "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a",
            ),
            (
                &b"abc"[..],
                "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532",
            ),
            (
                &b"The quick brown fox jumps over the lazy dog"[..],
                "69070dda01975c8c120c3aada1b282394e7f032fa9cf32f4cb2259a0897dfc04",
            ),
        ];
        for &(input, expected) in cases {
            assert_eq!(crate::hex::encode(hash(input)), expected);
        }
    }
}

use core::cmp::min;

use super::{KeyDerivationError, KeyDeriver};

const HASH_LEN: usize = 32;
const BLOCK_SIZE: usize = 64;

#[derive(Clone, Default)]
pub struct InhouseKeyDeriver {
    salt: Option<Vec<u8>>,
}

impl InhouseKeyDeriver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_salt<S: AsRef<[u8]>>(salt: S) -> Self {
        Self {
            salt: Some(salt.as_ref().to_vec()),
        }
    }
}

impl KeyDeriver for InhouseKeyDeriver {
    fn derive_key(
        &self,
        context: &[u8],
        material: &[u8],
    ) -> Result<[u8; HASH_LEN], KeyDerivationError> {
        let ctx = core::str::from_utf8(context).map_err(|_| KeyDerivationError::InvalidContext)?;
        let mut out = [0u8; HASH_LEN];
        derive_key_material(self.salt.as_deref(), ctx.as_bytes(), material, &mut out);
        Ok(out)
    }
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; HASH_LEN] {
    derive_key_with_info(context.as_bytes(), material)
}

pub fn derive_key_with_info(info: &[u8], material: &[u8]) -> [u8; HASH_LEN] {
    let mut out = [0u8; HASH_LEN];
    derive_key_material(None, info, material, &mut out);
    out
}

pub fn derive_key_with_salt(salt: &[u8], context: &str, material: &[u8]) -> [u8; HASH_LEN] {
    let mut out = [0u8; HASH_LEN];
    derive_key_material(Some(salt), context.as_bytes(), material, &mut out);
    out
}

pub fn derive_key_material(salt: Option<&[u8]>, info: &[u8], material: &[u8], output: &mut [u8]) {
    assert!(
        output.len() <= 255 * HASH_LEN,
        "hkdf output length too large"
    );
    let prk = hkdf_extract(salt, material);
    hkdf_expand_into(&prk, info, output);
}

fn hkdf_extract(salt: Option<&[u8]>, ikm: &[u8]) -> [u8; HASH_LEN] {
    let zero_salt = [0u8; BLOCK_SIZE];
    let key = salt.unwrap_or(&zero_salt);
    hmac_sha256(key, ikm)
}

fn hkdf_expand_into(prk: &[u8; HASH_LEN], info: &[u8], output: &mut [u8]) {
    let mut counter = 1u8;
    let mut generated = 0usize;
    let mut prev = [0u8; HASH_LEN];
    let mut prev_len = 0usize;

    while generated < output.len() {
        let mut buffer = Vec::with_capacity(prev_len + info.len() + 1);
        buffer.extend_from_slice(&prev[..prev_len]);
        buffer.extend_from_slice(info);
        buffer.push(counter);
        let block = hmac_sha256(prk, &buffer);
        prev.copy_from_slice(&block);
        prev_len = HASH_LEN;

        let take = min(HASH_LEN, output.len() - generated);
        output[generated..generated + take].copy_from_slice(&block[..take]);
        generated += take;
        counter = counter.wrapping_add(1);
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; HASH_LEN] {
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = sha256::Sha256::digest(key);
        key_block[..HASH_LEN].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0u8; BLOCK_SIZE];
    let mut outer_pad = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        let byte = key_block[i];
        inner_pad[i] = byte ^ 0x36;
        outer_pad[i] = byte ^ 0x5c;
    }

    let inner = sha256::Sha256::digest_chunks(&[&inner_pad, data]);
    sha256::Sha256::digest_chunks(&[&outer_pad, &inner])
}

mod sha256 {
    const BLOCK_SIZE: usize = 64;

    pub struct Sha256;

    impl Sha256 {
        pub fn digest(input: &[u8]) -> [u8; 32] {
            Self::digest_chunks(&[input])
        }

        pub fn digest_chunks(chunks: &[&[u8]]) -> [u8; 32] {
            let mut state = State::new();
            for chunk in chunks {
                state.update(chunk);
            }
            state.finalize()
        }
    }

    struct State {
        h: [u32; 8],
        buffer: [u8; BLOCK_SIZE],
        buffer_len: usize,
        bit_len: u64,
    }

    impl State {
        fn new() -> Self {
            Self {
                h: INITIAL_HASH,
                buffer: [0u8; BLOCK_SIZE],
                buffer_len: 0,
                bit_len: 0,
            }
        }

        fn update(&mut self, mut data: &[u8]) {
            if data.is_empty() {
                return;
            }

            self.bit_len = self.bit_len.wrapping_add((data.len() as u64) << 3);

            if self.buffer_len > 0 {
                let space = BLOCK_SIZE - self.buffer_len;
                if data.len() >= space {
                    self.buffer[self.buffer_len..self.buffer_len + space]
                        .copy_from_slice(&data[..space]);
                    let block = self.buffer;
                    self.process_block(&block);
                    self.buffer_len = 0;
                    data = &data[space..];
                } else {
                    self.buffer[self.buffer_len..self.buffer_len + data.len()]
                        .copy_from_slice(data);
                    self.buffer_len += data.len();
                    return;
                }
            }

            while data.len() >= BLOCK_SIZE {
                let mut block = [0u8; BLOCK_SIZE];
                block.copy_from_slice(&data[..BLOCK_SIZE]);
                self.process_block(&block);
                data = &data[BLOCK_SIZE..];
            }

            if !data.is_empty() {
                self.buffer[..data.len()].copy_from_slice(data);
                self.buffer_len = data.len();
            }
        }

        fn finalize(mut self) -> [u8; 32] {
            self.buffer[self.buffer_len] = 0x80;
            self.buffer_len += 1;

            if self.buffer_len > BLOCK_SIZE - 8 {
                for byte in &mut self.buffer[self.buffer_len..] {
                    *byte = 0;
                }
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }

            for byte in &mut self.buffer[self.buffer_len..BLOCK_SIZE - 8] {
                *byte = 0;
            }

            let bit_len_bytes = self.bit_len.to_be_bytes();
            self.buffer[BLOCK_SIZE - 8..BLOCK_SIZE].copy_from_slice(&bit_len_bytes);
            let block = self.buffer;
            self.process_block(&block);

            let mut out = [0u8; 32];
            for (chunk, value) in out.chunks_mut(4).zip(self.h.iter()) {
                chunk.copy_from_slice(&value.to_be_bytes());
            }
            out
        }

        fn process_block(&mut self, block: &[u8; BLOCK_SIZE]) {
            let mut w = [0u32; 64];
            for (i, chunk) in block.chunks_exact(4).enumerate().take(16) {
                w[i] = u32::from_be_bytes(chunk.try_into().expect("chunk"));
            }

            for t in 16..64 {
                let s0 = small_sigma0(w[t - 15]);
                let s1 = small_sigma1(w[t - 2]);
                w[t] = w[t - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[t - 7])
                    .wrapping_add(s1);
            }

            let mut a = self.h[0];
            let mut b = self.h[1];
            let mut c = self.h[2];
            let mut d = self.h[3];
            let mut e = self.h[4];
            let mut f = self.h[5];
            let mut g = self.h[6];
            let mut h = self.h[7];

            for t in 0..64 {
                let t1 = h
                    .wrapping_add(big_sigma1(e))
                    .wrapping_add(ch(e, f, g))
                    .wrapping_add(K[t])
                    .wrapping_add(w[t]);
                let t2 = big_sigma0(a).wrapping_add(maj(a, b, c));

                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }

            self.h[0] = self.h[0].wrapping_add(a);
            self.h[1] = self.h[1].wrapping_add(b);
            self.h[2] = self.h[2].wrapping_add(c);
            self.h[3] = self.h[3].wrapping_add(d);
            self.h[4] = self.h[4].wrapping_add(e);
            self.h[5] = self.h[5].wrapping_add(f);
            self.h[6] = self.h[6].wrapping_add(g);
            self.h[7] = self.h[7].wrapping_add(h);
        }
    }

    #[inline(always)]
    fn ch(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ ((!x) & z)
    }

    #[inline(always)]
    fn maj(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ (x & z) ^ (y & z)
    }

    #[inline(always)]
    fn big_sigma0(x: u32) -> u32 {
        x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
    }

    #[inline(always)]
    fn big_sigma1(x: u32) -> u32 {
        x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
    }

    #[inline(always)]
    fn small_sigma0(x: u32) -> u32 {
        x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
    }

    #[inline(always)]
    fn small_sigma1(x: u32) -> u32 {
        x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
    }

    const INITIAL_HASH: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
}

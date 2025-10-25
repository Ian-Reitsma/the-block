#![forbid(unsafe_code)]

use crate::Result;

pub const OUTPUT_SIZE: usize = 20;

/// Compute the SHA-1 digest of the provided data slice.
pub fn hash(data: &[u8]) -> Result<[u8; OUTPUT_SIZE]> {
    let mut state = Sha1::new();
    state.update(data);
    Ok(state.finalize())
}

#[derive(Clone)]
struct Sha1 {
    state: [u32; 5],
    bit_len: u64,
    buffer: [u8; 64],
    buffer_len: usize,
}

impl Sha1 {
    const fn new() -> Self {
        Self {
            state: [
                0x6745_2301,
                0xEFCD_AB89,
                0x98BA_DCFE,
                0x1032_5476,
                0xC3D2_E1F0,
            ],
            bit_len: 0,
            buffer: [0u8; 64],
            buffer_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        if data.is_empty() {
            return;
        }

        // Update total length (in bits) with checked arithmetic and wrap on overflow as per
        // the SHA-1 specification when processing more than 2^64 bits.
        self.bit_len = self.bit_len.wrapping_add((data.len() as u64) << 3);

        if self.buffer_len != 0 {
            let missing = 64 - self.buffer_len;
            let take = missing.min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&data[..take]);
            self.buffer_len += take;
            data = &data[take..];

            if self.buffer_len == 64 {
                let mut block = [0u8; 64];
                block.copy_from_slice(&self.buffer);
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.process_block(&block);
            data = &data[64..];
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    fn finalize(mut self) -> [u8; OUTPUT_SIZE] {
        let mut block = [0u8; 64];

        // Copy existing buffered data into the block.
        if self.buffer_len != 0 {
            block[..self.buffer_len].copy_from_slice(&self.buffer[..self.buffer_len]);
        }

        // Append the padding bit.
        block[self.buffer_len] = 0x80;

        if self.buffer_len >= 56 {
            // Not enough room for length; process this block and use another one for length.
            self.process_block(&block);
            block = [0u8; 64];
        } else {
            for byte in block[self.buffer_len + 1..56].iter_mut() {
                *byte = 0;
            }
        }

        // Append total length (big-endian) in bits.
        block[56..].copy_from_slice(&self.bit_len.to_be_bytes());
        self.process_block(&block);

        let mut out = [0u8; OUTPUT_SIZE];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..80 {
            let xor = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
            w[i] = xor.rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_digest(input: &[u8], expected_hex: &str) {
        let digest = hash(input).expect("sha1 hash");
        let actual_hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(actual_hex, expected_hex);
    }

    #[test]
    fn hashes_empty_message() {
        assert_digest(b"", "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn hashes_short_message() {
        assert_digest(b"abc", "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn hashes_long_message() {
        let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_digest(input, "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
    }

    #[test]
    fn hashes_multi_block_padding() {
        let input = vec![0u8; 1_000];
        assert_digest(&input, "c577f7a37657053275f3e3ecc06ec22e6b909366");
    }
}

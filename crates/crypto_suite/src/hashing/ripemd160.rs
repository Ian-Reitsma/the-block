#![forbid(unsafe_code)]

use crate::Result;

pub const OUTPUT_SIZE: usize = 20;

/// Compute the RIPEMD-160 digest of the provided data.
pub fn hash(data: &[u8]) -> Result<[u8; OUTPUT_SIZE]> {
    let mut state = Ripemd160::new();
    state.update(data);
    Ok(state.finalize())
}

#[derive(Clone)]
struct Ripemd160 {
    state: [u32; 5],
    bit_len: u64,
    buffer: [u8; 64],
    buffer_len: usize,
}

impl Ripemd160 {
    const RL: [usize; 80] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9,
        5, 2, 14, 11, 8, 3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12, 1, 9, 11, 10, 0, 8,
        12, 4, 13, 3, 7, 15, 14, 5, 6, 2, 4, 0, 5, 9, 7, 12, 2, 10, 14, 1, 3, 8, 11, 6, 15, 13,
    ];

    const RR: [usize; 80] = [
        5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12, 6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8,
        12, 4, 9, 1, 2, 15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13, 8, 6, 4, 1, 3, 11,
        15, 0, 5, 12, 2, 13, 9, 7, 10, 14, 12, 15, 10, 4, 1, 5, 8, 7, 6, 2, 13, 14, 0, 3, 9, 11,
    ];

    const SL: [u32; 80] = [
        11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8, 7, 6, 8, 13, 11, 9, 7, 15, 7, 12,
        15, 9, 11, 7, 13, 12, 11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5, 11, 12, 14,
        15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12, 9, 15, 5, 11, 6, 8, 13, 12, 5, 12, 13, 14, 11,
        8, 5, 6,
    ];

    const SR: [u32; 80] = [
        8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6, 9, 13, 15, 7, 12, 8, 9, 11, 7, 7,
        12, 7, 6, 15, 13, 11, 9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5, 15, 5, 8, 11,
        14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8, 8, 5, 12, 9, 12, 5, 14, 6, 8, 13, 6, 5, 15, 13,
        11, 11,
    ];

    const KL: [u32; 5] = [
        0x0000_0000,
        0x5A82_7999,
        0x6ED9_EBA1,
        0x8F1B_BCDC,
        0xA953_FD4E,
    ];
    const KR: [u32; 5] = [
        0x50A2_8BE6,
        0x5C4D_D124,
        0x6D70_3EF3,
        0x7A6D_76E9,
        0x0000_0000,
    ];

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
        if self.buffer_len != 0 {
            block[..self.buffer_len].copy_from_slice(&self.buffer[..self.buffer_len]);
        }

        block[self.buffer_len] = 0x80;

        if self.buffer_len >= 56 {
            self.process_block(&block);
            block = [0u8; 64];
        } else {
            for byte in block[self.buffer_len + 1..56].iter_mut() {
                *byte = 0;
            }
        }

        block[56..].copy_from_slice(&self.bit_len.to_le_bytes());
        self.process_block(&block);

        let mut out = [0u8; OUTPUT_SIZE];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut words = [0u32; 16];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            words[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        let mut al = self.state[0];
        let mut bl = self.state[1];
        let mut cl = self.state[2];
        let mut dl = self.state[3];
        let mut el = self.state[4];

        let mut ar = self.state[0];
        let mut br = self.state[1];
        let mut cr = self.state[2];
        let mut dr = self.state[3];
        let mut er = self.state[4];

        for i in 0..80 {
            let round = i / 16;
            let temp_l = al
                .wrapping_add(f_left(i, bl, cl, dl))
                .wrapping_add(words[Self::RL[i]])
                .wrapping_add(Self::KL[round]);
            let temp_l = temp_l.rotate_left(Self::SL[i]).wrapping_add(el);
            al = el;
            el = dl;
            dl = cl.rotate_left(10);
            cl = bl;
            bl = temp_l;

            let temp_r = ar
                .wrapping_add(f_right(i, br, cr, dr))
                .wrapping_add(words[Self::RR[i]])
                .wrapping_add(Self::KR[round]);
            let temp_r = temp_r.rotate_left(Self::SR[i]).wrapping_add(er);
            ar = er;
            er = dr;
            dr = cr.rotate_left(10);
            cr = br;
            br = temp_r;
        }

        let temp = self.state[1].wrapping_add(cl).wrapping_add(dr);
        self.state[1] = self.state[2].wrapping_add(dl).wrapping_add(er);
        self.state[2] = self.state[3].wrapping_add(el).wrapping_add(ar);
        self.state[3] = self.state[4].wrapping_add(al).wrapping_add(br);
        self.state[4] = self.state[0].wrapping_add(bl).wrapping_add(cr);
        self.state[0] = temp;
    }
}

#[inline]
fn f_left(i: usize, x: u32, y: u32, z: u32) -> u32 {
    match i {
        0..=15 => x ^ y ^ z,
        16..=31 => (x & y) | ((!x) & z),
        32..=47 => (x | !y) ^ z,
        48..=63 => (x & z) | (y & !z),
        _ => x ^ (y | !z),
    }
}

#[inline]
fn f_right(i: usize, x: u32, y: u32, z: u32) -> u32 {
    match i {
        0..=15 => x ^ (y | !z),
        16..=31 => (x & z) | (y & !z),
        32..=47 => (x | !y) ^ z,
        48..=63 => (x & y) | ((!x) & z),
        _ => x ^ y ^ z,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_digest(input: &[u8], expected_hex: &str) {
        let digest = hash(input).expect("ripemd160 hash");
        let actual_hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(actual_hex, expected_hex);
    }

    #[test]
    fn hashes_empty_message() {
        assert_digest(b"", "9c1185a5c5e9fc54612808977ee8f548b2258d31");
    }

    #[test]
    fn hashes_short_message() {
        assert_digest(b"abc", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
    }

    #[test]
    fn hashes_known_phrase() {
        assert_digest(
            b"message digest",
            "5d0689ef49d2fae572b881b123a85ffa21595f36",
        );
    }

    #[test]
    fn hashes_multi_block_input() {
        let input = [0x61u8; 128];
        assert_digest(&input, "8dfdfb32b2ed5cb41a73478b4fd60cc5b4648b15");
    }
}

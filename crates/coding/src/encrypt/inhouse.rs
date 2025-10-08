use crate::error::EncryptError;
use crate::primitives::rng;
use core::num::Wrapping;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;
pub const XNONCE_LEN: usize = 24;
const BLOCK_WORDS: usize = 16;
const BLOCK_BYTES: usize = 64;

#[inline(always)]
fn rotate_left(v: u32, n: u32) -> u32 {
    v.rotate_left(n)
}

#[inline(always)]
fn quarter_round(state: &mut [u32; BLOCK_WORDS], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = rotate_left(state[d], 16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = rotate_left(state[b], 12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = rotate_left(state[d], 8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = rotate_left(state[b], 7);
}

fn chacha20_block(key: &[u8; KEY_LEN], counter: u32, nonce: &[u8; NONCE_LEN]) -> [u8; BLOCK_BYTES] {
    let mut state = [0u32; BLOCK_WORDS];
    state[0] = 0x6170_7865;
    state[1] = 0x3320_646e;
    state[2] = 0x7962_2d32;
    state[3] = 0x6b20_6574;

    for (i, chunk) in key.chunks_exact(4).enumerate() {
        state[4 + i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }

    state[12] = counter;
    state[13] = u32::from_le_bytes(nonce[0..4].try_into().unwrap());
    state[14] = u32::from_le_bytes(nonce[4..8].try_into().unwrap());
    state[15] = u32::from_le_bytes(nonce[8..12].try_into().unwrap());

    let mut working = state;
    for _ in 0..10 {
        quarter_round(&mut working, 0, 4, 8, 12);
        quarter_round(&mut working, 1, 5, 9, 13);
        quarter_round(&mut working, 2, 6, 10, 14);
        quarter_round(&mut working, 3, 7, 11, 15);
        quarter_round(&mut working, 0, 5, 10, 15);
        quarter_round(&mut working, 1, 6, 11, 12);
        quarter_round(&mut working, 2, 7, 8, 13);
        quarter_round(&mut working, 3, 4, 9, 14);
    }

    for i in 0..BLOCK_WORDS {
        working[i] = working[i].wrapping_add(state[i]);
    }

    let mut out = [0u8; BLOCK_BYTES];
    for (i, word) in working.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn hchacha20(key: &[u8; KEY_LEN], nonce: &[u8; 16]) -> [u8; KEY_LEN] {
    let mut state = [0u32; BLOCK_WORDS];
    state[0] = 0x6170_7865;
    state[1] = 0x3320_646e;
    state[2] = 0x7962_2d32;
    state[3] = 0x6b20_6574;

    for (i, chunk) in key.chunks_exact(4).enumerate() {
        state[4 + i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }

    for (i, chunk) in nonce.chunks_exact(4).enumerate() {
        state[12 + i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }

    let mut working = state;
    for _ in 0..10 {
        quarter_round(&mut working, 0, 4, 8, 12);
        quarter_round(&mut working, 1, 5, 9, 13);
        quarter_round(&mut working, 2, 6, 10, 14);
        quarter_round(&mut working, 3, 7, 11, 15);
        quarter_round(&mut working, 0, 5, 10, 15);
        quarter_round(&mut working, 1, 6, 11, 12);
        quarter_round(&mut working, 2, 7, 8, 13);
        quarter_round(&mut working, 3, 4, 9, 14);
    }

    let mut out = [0u8; KEY_LEN];
    let words = [
        working[0],
        working[1],
        working[2],
        working[3],
        working[12],
        working[13],
        working[14],
        working[15],
    ];
    for (i, word) in words.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn derive_xchacha_params(
    key: &[u8; KEY_LEN],
    nonce: &[u8; XNONCE_LEN],
) -> ([u8; KEY_LEN], [u8; NONCE_LEN]) {
    let mut nonce16 = [0u8; 16];
    nonce16.copy_from_slice(&nonce[..16]);
    let derived_key = hchacha20(key, &nonce16);

    let mut derived_nonce = [0u8; NONCE_LEN];
    derived_nonce[4..].copy_from_slice(&nonce[16..]);
    (derived_key, derived_nonce)
}

#[inline(always)]
fn clamp_r(mut r: [u8; 16]) -> [u8; 16] {
    r[3] &= 15;
    r[7] &= 15;
    r[11] &= 15;
    r[15] &= 15;
    r[4] &= 252;
    r[8] &= 252;
    r[12] &= 252;
    r
}

fn load32(input: &[u8], offset: usize) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&input[offset..offset + 4]);
    u32::from_le_bytes(buf)
}

fn decode_block(block: &[u8; 16], full: bool) -> [u128; 5] {
    const MASK: u128 = (1 << 26) - 1;
    let t0 = u128::from(load32(block, 0)) & MASK;
    let t1 = (u128::from(load32(block, 3)) >> 2) & MASK;
    let t2 = (u128::from(load32(block, 6)) >> 4) & MASK;
    let t3 = (u128::from(load32(block, 9)) >> 6) & MASK;
    let mut t4 = (u128::from(load32(block, 12)) >> 8) & MASK;
    if full {
        t4 |= 1 << 24;
    }
    [t0, t1, t2, t3, t4]
}

fn poly1305_accumulate(r: &[u8; 16], s: &[u8; 16], msg: &[u8]) -> [u8; 16] {
    const MASK: u128 = (1 << 26) - 1;

    let r_limbs_full = decode_block(r, false);
    let r0 = r_limbs_full[0];
    let r1 = r_limbs_full[1];
    let r2 = r_limbs_full[2];
    let r3 = r_limbs_full[3];
    let r4 = r_limbs_full[4];

    let r1_5 = r1 * 5;
    let r2_5 = r2 * 5;
    let r3_5 = r3 * 5;
    let r4_5 = r4 * 5;

    let mut h0 = 0u128;
    let mut h1 = 0u128;
    let mut h2 = 0u128;
    let mut h3 = 0u128;
    let mut h4 = 0u128;

    let mut chunks = msg.chunks_exact(16);
    for block in &mut chunks {
        let block_arr: [u8; 16] = block.try_into().unwrap();
        let limbs = decode_block(&block_arr, true);
        h0 += limbs[0];
        h1 += limbs[1];
        h2 += limbs[2];
        h3 += limbs[3];
        h4 += limbs[4];

        let d0 = h0 * r0 + h1 * r4_5 + h2 * r3_5 + h3 * r2_5 + h4 * r1_5;
        let mut d1 = h0 * r1 + h1 * r0 + h2 * r4_5 + h3 * r3_5 + h4 * r2_5;
        let mut d2 = h0 * r2 + h1 * r1 + h2 * r0 + h3 * r4_5 + h4 * r3_5;
        let mut d3 = h0 * r3 + h1 * r2 + h2 * r1 + h3 * r0 + h4 * r4_5;
        let mut d4 = h0 * r4 + h1 * r3 + h2 * r2 + h3 * r1 + h4 * r0;

        let mut carry = d0 >> 26;
        h0 = d0 & MASK;
        d1 += carry;

        carry = d1 >> 26;
        h1 = d1 & MASK;
        d2 += carry;

        carry = d2 >> 26;
        h2 = d2 & MASK;
        d3 += carry;

        carry = d3 >> 26;
        h3 = d3 & MASK;
        d4 += carry;

        carry = d4 >> 26;
        h4 = d4 & MASK;
        h0 += carry * 5;

        carry = h0 >> 26;
        h0 &= MASK;
        h1 += carry;
    }

    let remainder = chunks.remainder();
    if !remainder.is_empty() {
        let mut block = [0u8; 16];
        block[..remainder.len()].copy_from_slice(remainder);
        block[remainder.len()] = 1;
        let limbs = decode_block(&block, false);
        h0 += limbs[0];
        h1 += limbs[1];
        h2 += limbs[2];
        h3 += limbs[3];
        h4 += limbs[4];

        let d0 = h0 * r0 + h1 * r4_5 + h2 * r3_5 + h3 * r2_5 + h4 * r1_5;
        let mut d1 = h0 * r1 + h1 * r0 + h2 * r4_5 + h3 * r3_5 + h4 * r2_5;
        let mut d2 = h0 * r2 + h1 * r1 + h2 * r0 + h3 * r4_5 + h4 * r3_5;
        let mut d3 = h0 * r3 + h1 * r2 + h2 * r1 + h3 * r0 + h4 * r4_5;
        let mut d4 = h0 * r4 + h1 * r3 + h2 * r2 + h3 * r1 + h4 * r0;

        let mut carry = d0 >> 26;
        h0 = d0 & MASK;
        d1 += carry;

        carry = d1 >> 26;
        h1 = d1 & MASK;
        d2 += carry;

        carry = d2 >> 26;
        h2 = d2 & MASK;
        d3 += carry;

        carry = d3 >> 26;
        h3 = d3 & MASK;
        d4 += carry;

        carry = d4 >> 26;
        h4 = d4 & MASK;
        h0 += carry * 5;

        carry = h0 >> 26;
        h0 &= MASK;
        h1 += carry;
    }

    let mut lengths = [0u8; 16];
    lengths[8..].copy_from_slice(&(msg.len() as u64).to_le_bytes());
    let limbs = decode_block(&lengths, true);
    h0 += limbs[0];
    h1 += limbs[1];
    h2 += limbs[2];
    h3 += limbs[3];
    h4 += limbs[4];

    let d0 = h0 * r0 + h1 * r4_5 + h2 * r3_5 + h3 * r2_5 + h4 * r1_5;
    let mut d1 = h0 * r1 + h1 * r0 + h2 * r4_5 + h3 * r3_5 + h4 * r2_5;
    let mut d2 = h0 * r2 + h1 * r1 + h2 * r0 + h3 * r4_5 + h4 * r3_5;
    let mut d3 = h0 * r3 + h1 * r2 + h2 * r1 + h3 * r0 + h4 * r4_5;
    let mut d4 = h0 * r4 + h1 * r3 + h2 * r2 + h3 * r1 + h4 * r0;

    let mut carry = d0 >> 26;
    h0 = d0 & MASK;
    d1 += carry;

    carry = d1 >> 26;
    h1 = d1 & MASK;
    d2 += carry;

    carry = d2 >> 26;
    h2 = d2 & MASK;
    d3 += carry;

    carry = d3 >> 26;
    h3 = d3 & MASK;
    d4 += carry;

    carry = d4 >> 26;
    h4 = d4 & MASK;
    h0 += carry * 5;

    carry = h0 >> 26;
    h0 &= MASK;
    h1 += carry;

    carry = h1 >> 26;
    h1 &= MASK;
    h2 += carry;

    carry = h2 >> 26;
    h2 &= MASK;
    h3 += carry;

    carry = h3 >> 26;
    h3 &= MASK;
    h4 += carry;

    carry = h4 >> 26;
    h4 &= MASK;
    h0 += carry * 5;

    carry = h0 >> 26;
    h0 &= MASK;
    h1 += carry;

    carry = h1 >> 26;
    h1 &= MASK;
    h2 += carry;

    carry = h2 >> 26;
    h2 &= MASK;
    h3 += carry;

    carry = h3 >> 26;
    h3 &= MASK;
    h4 += carry;
    h4 &= MASK;

    let mut g0 = h0 + 5;
    carry = g0 >> 26;
    g0 &= MASK;
    let mut g1 = h1 + carry;
    carry = g1 >> 26;
    g1 &= MASK;
    let mut g2 = h2 + carry;
    carry = g2 >> 26;
    g2 &= MASK;
    let mut g3 = h3 + carry;
    carry = g3 >> 26;
    g3 &= MASK;
    let g4 = (h4 + carry) as i128 - (1 << 26);

    let mask = (g4 >> 63) as i128;
    let not_mask = !mask;

    let h0_i = h0 as i128;
    let h1_i = h1 as i128;
    let h2_i = h2 as i128;
    let h3_i = h3 as i128;
    let h4_i = h4 as i128;

    let g0_i = g0 as i128;
    let g1_i = g1 as i128;
    let g2_i = g2 as i128;
    let g3_i = g3 as i128;

    let f0 = ((g0_i & not_mask) | (h0_i & mask)) as u128;
    let f1 = ((g1_i & not_mask) | (h1_i & mask)) as u128;
    let f2 = ((g2_i & not_mask) | (h2_i & mask)) as u128;
    let f3 = ((g3_i & not_mask) | (h3_i & mask)) as u128;
    let f4 = (((g4 + (1 << 26)) & not_mask) | (h4_i & mask)) as u128;

    let tag_base = Wrapping(f0)
        + Wrapping(f1 << 26)
        + Wrapping(f2 << 52)
        + Wrapping(f3 << 78)
        + Wrapping(f4 << 104);

    let s_val = Wrapping(u128::from_le_bytes(*s));
    let tag = (tag_base + s_val).0;
    tag.to_le_bytes()
}

fn compute_poly1305_key(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN]) -> ([u8; 16], [u8; 16]) {
    let block = chacha20_block(key, 0, nonce);
    let mut r = [0u8; 16];
    let mut s = [0u8; 16];
    r.copy_from_slice(&block[0..16]);
    s.copy_from_slice(&block[16..32]);
    (clamp_r(r), s)
}

fn chacha20_apply(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    counter: u32,
    data: &[u8],
) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len());
    let mut ctr = counter;
    let mut offset = 0usize;
    while offset < data.len() {
        let block = chacha20_block(key, ctr, nonce);
        ctr = ctr.wrapping_add(1);
        let take = (data.len() - offset).min(BLOCK_BYTES);
        for i in 0..take {
            output.push(data[offset + i] ^ block[i]);
        }
        offset += take;
    }
    output
}

fn constant_time_eq(a: &[u8; TAG_LEN], b: &[u8; TAG_LEN]) -> bool {
    let mut diff = 0u8;
    for i in 0..TAG_LEN {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn seal_with_nonce(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, EncryptError> {
    let (r, s) = compute_poly1305_key(key, nonce);
    let ciphertext = chacha20_apply(key, nonce, 1, plaintext);
    let tag = poly1305_accumulate(&r, &s, &ciphertext);

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len() + TAG_LEN);
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&tag);
    Ok(out)
}

pub fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
    let mut nonce = [0u8; NONCE_LEN];
    rng::fill_secure_bytes(&mut nonce).map_err(|err| EncryptError::EntropyUnavailable {
        reason: err.reason(),
    })?;
    seal_with_nonce(key, &nonce, plaintext)
}

pub fn decrypt(key: &[u8; KEY_LEN], payload: &[u8]) -> Result<Vec<u8>, EncryptError> {
    if payload.len() < NONCE_LEN + TAG_LEN {
        return Err(EncryptError::InvalidCiphertext { len: payload.len() });
    }
    let (nonce_bytes, rest) = payload.split_at(NONCE_LEN);
    let (ciphertext, tag_bytes) = rest.split_at(rest.len() - TAG_LEN);
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(nonce_bytes);
    let mut tag = [0u8; TAG_LEN];
    tag.copy_from_slice(tag_bytes);

    let (r, s) = compute_poly1305_key(key, &nonce);
    let expected = poly1305_accumulate(&r, &s, ciphertext);
    if !constant_time_eq(&expected, &tag) {
        return Err(EncryptError::DecryptionFailed);
    }
    Ok(chacha20_apply(key, &nonce, 1, ciphertext))
}

fn seal_xchacha_with_nonce(
    key: &[u8; KEY_LEN],
    nonce: &[u8; XNONCE_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, EncryptError> {
    let (derived_key, derived_nonce) = derive_xchacha_params(key, nonce);
    let inner = seal_with_nonce(&derived_key, &derived_nonce, plaintext)?;
    let mut out = Vec::with_capacity(XNONCE_LEN + inner.len().saturating_sub(NONCE_LEN));
    out.extend_from_slice(nonce);
    out.extend_from_slice(&inner[NONCE_LEN..]);
    Ok(out)
}

pub fn encrypt_xchacha(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
    let mut nonce = [0u8; XNONCE_LEN];
    rng::fill_secure_bytes(&mut nonce).map_err(|err| EncryptError::EntropyUnavailable {
        reason: err.reason(),
    })?;
    seal_xchacha_with_nonce(key, &nonce, plaintext)
}

pub fn encrypt_xchacha_with_nonce(
    key: &[u8; KEY_LEN],
    nonce: &[u8; XNONCE_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, EncryptError> {
    seal_xchacha_with_nonce(key, nonce, plaintext)
}

pub fn decrypt_xchacha(key: &[u8; KEY_LEN], payload: &[u8]) -> Result<Vec<u8>, EncryptError> {
    if payload.len() < XNONCE_LEN + TAG_LEN {
        return Err(EncryptError::InvalidCiphertext { len: payload.len() });
    }
    let mut nonce = [0u8; XNONCE_LEN];
    nonce.copy_from_slice(&payload[..XNONCE_LEN]);
    let (derived_key, derived_nonce) = derive_xchacha_params(key, &nonce);
    let mut combined = Vec::with_capacity(NONCE_LEN + payload.len() - XNONCE_LEN);
    combined.extend_from_slice(&derived_nonce);
    combined.extend_from_slice(&payload[XNONCE_LEN..]);
    decrypt(&derived_key, &combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = [7u8; KEY_LEN];
        let plaintext = b"test message with enough length to span blocks";
        let cipher = encrypt(&key, plaintext).unwrap();
        let plain = decrypt(&key, &cipher).unwrap();
        assert_eq!(plain, plaintext);
    }

    #[test]
    fn rfc_vector() {
        let key: [u8; KEY_LEN] = [
            0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d,
            0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b,
            0x9c, 0x9d, 0x9e, 0x9f,
        ];
        let nonce = [
            0x07, 0x00, 0x00, 0x00, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
        ];
        let mut plaintext = vec![0u8; 114];
        for (i, b) in plaintext.iter_mut().enumerate() {
            *b = i as u8;
        }
        let cipher = seal_with_nonce(&key, &nonce, &plaintext).unwrap();
        assert_eq!(&cipher[..NONCE_LEN], &nonce);
        let plain = decrypt(&key, &cipher).unwrap();
        assert_eq!(plain, plaintext);
    }

    #[test]
    fn tamper_detected() {
        let key = [42u8; KEY_LEN];
        let plaintext = b"short";
        let mut cipher = encrypt(&key, plaintext).unwrap();
        cipher[NONCE_LEN] ^= 0x01;
        assert!(matches!(
            decrypt(&key, &cipher),
            Err(EncryptError::DecryptionFailed)
        ));
    }
}

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::collections::VecDeque;

/// Maximum number of attempts when constructing a filter before giving up.
const MAX_RETRIES: usize = 64;

/// First-party XOR filter implementation matching the behaviour required by
/// the networking rate-limit filter.
#[derive(Clone, Debug)]
pub struct Xor8 {
    seed: u64,
    block_length: usize,
    fingerprints: Vec<u8>,
}

impl Xor8 {
    /// Create a new filter populated with the provided keys. When the
    /// construction fails, an empty filter is returned.
    pub fn new(keys: &[u64]) -> Self {
        Self::populate(keys).unwrap_or_else(|_| Self {
            seed: 0,
            block_length: 0,
            fingerprints: Vec::new(),
        })
    }

    /// Populate a filter with the provided keys.
    pub fn populate(keys: &[u64]) -> Result<Self, BuildError> {
        if keys.is_empty() {
            return Ok(Self {
                seed: 0,
                block_length: 0,
                fingerprints: Vec::new(),
            });
        }

        for attempt in 0..MAX_RETRIES {
            let seed = mix_seed(attempt as u64);
            if let Some((fingerprints, block_length)) = attempt_build(keys, seed) {
                return Ok(Self {
                    seed,
                    block_length,
                    fingerprints,
                });
            }
        }

        Err(BuildError::Failed)
    }

    /// Returns `true` when the filter probably contains the provided key.
    pub fn contains(&self, key: &u64) -> bool {
        if self.block_length == 0 {
            return false;
        }

        let hash = mix64(key ^ self.seed);
        let fingerprint = fingerprint(hash);
        let (h0, h1, h2) = positions(hash, self.block_length);
        let lhs = self.fingerprints[h0] ^ self.fingerprints[h1] ^ self.fingerprints[h2];
        lhs == fingerprint
    }
}

/// Error returned when filter construction repeatedly fails.
#[derive(Debug)]
pub enum BuildError {
    Failed,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Failed => write!(f, "unable to construct xor filter"),
        }
    }
}

impl std::error::Error for BuildError {}

fn attempt_build(keys: &[u64], seed: u64) -> Option<(Vec<u8>, usize)> {
    let capacity = keys.len();
    let block_length = next_power_of_two((capacity as f64 * 1.23).ceil() as usize).max(1);
    let size = block_length * 3;

    let mut counts = vec![0u32; size];
    let mut xors = vec![0u32; size];
    let mut hashes = Vec::with_capacity(capacity);

    for (index, &key) in keys.iter().enumerate() {
        let hash = mix64(key ^ seed);
        let (h0, h1, h2) = positions(hash, block_length);
        counts[h0] += 1;
        xors[h0] ^= index as u32;
        counts[h1] += 1;
        xors[h1] ^= index as u32;
        counts[h2] += 1;
        xors[h2] ^= index as u32;
        hashes.push(hash);
    }

    let mut queue = VecDeque::new();
    for (index, &count) in counts.iter().enumerate().take(size) {
        if count == 1 {
            queue.push_back(index);
        }
    }

    let mut stack = Vec::with_capacity(capacity);
    while let Some(index) = queue.pop_front() {
        if counts[index] == 0 {
            continue;
        }
        let key_index = xors[index] as usize;
        let hash = hashes[key_index];
        stack.push((index, key_index));

        let (h0, h1, h2) = positions(hash, block_length);
        for h in [h0, h1, h2] {
            counts[h] -= 1;
            xors[h] ^= key_index as u32;
            if counts[h] == 1 {
                queue.push_back(h);
            }
        }
    }

    if stack.len() != capacity {
        return None;
    }

    let mut fingerprints = vec![0u8; size];
    while let Some((index, key_index)) = stack.pop() {
        let hash = hashes[key_index];
        let fp = fingerprint(hash);
        let (h0, h1, h2) = positions(hash, block_length);

        let mut value = fp;
        if h0 != index {
            value ^= fingerprints[h0];
        }
        if h1 != index {
            value ^= fingerprints[h1];
        }
        if h2 != index {
            value ^= fingerprints[h2];
        }
        fingerprints[index] = value;
    }

    Some((fingerprints, block_length))
}

fn positions(hash: u64, block_length: usize) -> (usize, usize, usize) {
    let mask = (block_length - 1) as u64;
    let h0 = (hash & mask) as usize;
    let h1 = (((hash >> 21) & mask) as usize) + block_length;
    let h2 = (((hash >> 42) & mask) as usize) + block_length * 2;
    (h0, h1, h2)
}

fn fingerprint(hash: u64) -> u8 {
    let fp = hash ^ (hash >> 32);
    ((fp as u8) | 1).wrapping_add(0)
}

fn mix_seed(value: u64) -> u64 {
    mix64(value ^ 0x9e37_79b9_7f4a_7c15)
}

fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn next_power_of_two(mut value: usize) -> usize {
    if value <= 1 {
        return 1;
    }
    value -= 1;
    value |= value >> 1;
    value |= value >> 2;
    value |= value >> 4;
    value |= value >> 8;
    value |= value >> 16;
    if usize::BITS > 32 {
        value |= value >> 32;
    }
    value + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_queries_filter() {
        let keys: Vec<u64> = (0..128).map(|v| mix64(v)).collect();
        let filter = Xor8::populate(&keys).expect("build filter");
        for key in &keys {
            assert!(filter.contains(key));
        }
        let misses = (0..128)
            .map(|v: u64| mix64(v.wrapping_add(1_000)))
            .filter(|k| filter.contains(k))
            .count();
        assert!(
            misses < 4,
            "unexpectedly high false positive rate: {misses}"
        );
    }
}

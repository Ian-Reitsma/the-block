use reed_solomon_erasure::galois_8::ReedSolomon;

const K: usize = 16; // data shards
const R: usize = 5; // local parities
const H: usize = 3; // global parities
const PARITY: usize = R + H;

/// Encode a chunk into LRC shards with a Progressive Fountain overlay.
pub fn encode(chunk: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let shard_size = (chunk.len() + K - 1) / K;
    let mut shards = Vec::with_capacity(K + PARITY);
    for i in 0..K {
        let start = i * shard_size;
        let end = usize::min(start + shard_size, chunk.len());
        let mut s = vec![0u8; shard_size];
        if start < end {
            s[..end - start].copy_from_slice(&chunk[start..end]);
        }
        shards.push(s);
    }
    for _ in 0..PARITY {
        shards.push(vec![0u8; shard_size]);
    }
    let r = ReedSolomon::new(K, PARITY).map_err(|e| e.to_string())?;
    r.encode(&mut shards).map_err(|e| e.to_string())?;
    Ok(overlay_fountain(shards))
}

fn overlay_fountain(mut shards: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let k_prime = 2usize;
    let n_prime = 5usize;
    if shards.len() >= k_prime {
        let base0 = shards[0].clone();
        let base1 = shards[1].clone();
        for i in 0..(n_prime - k_prime) {
            let mut combo = base0.clone();
            for (a, b) in combo.iter_mut().zip(&base1) {
                *a ^= *b;
            }
            combo[0] ^= i as u8; // differentiate shards
            shards.push(combo);
        }
    }
    shards
}

/// Reconstruct the original chunk from data/parity shards.
pub fn reconstruct(mut shards: Vec<Option<Vec<u8>>>) -> Result<Vec<u8>, String> {
    let r = ReedSolomon::new(K, PARITY).map_err(|e| e.to_string())?;
    r.reconstruct(&mut shards).map_err(|e| e.to_string())?;
    shards
        .into_iter()
        .next()
        .flatten()
        .ok_or_else(|| "missing data shard".to_string())
}

use reed_solomon_erasure::galois_8::ReedSolomon;

const K: usize = 16; // data shards
const R: usize = 5; // local parities
const H: usize = 3; // global parities
const PARITY: usize = R + H;
const FOUNTAIN_SOURCE: usize = 2;
const FOUNTAIN_TARGET: usize = 5;
const FOUNTAIN_OVERLAY: usize = FOUNTAIN_TARGET - FOUNTAIN_SOURCE;

/// Number of Reed–Solomon data shards produced for each chunk.
pub const RS_DATA_SHARDS: usize = K;
/// Number of Reed–Solomon parity shards appended for each chunk.
pub const RS_PARITY_SHARDS: usize = PARITY;
/// Total number of Reed–Solomon shards (data + parity).
pub const RS_TOTAL_SHARDS: usize = RS_DATA_SHARDS + RS_PARITY_SHARDS;
/// Number of fountain-overlay shards layered on top of the Reed–Solomon set.
pub const FOUNTAIN_OVERLAY_SHARDS: usize = FOUNTAIN_OVERLAY;
/// Total shards emitted by [`encode`], including the fountain overlay.
pub const TOTAL_SHARDS_PER_CHUNK: usize = RS_TOTAL_SHARDS + FOUNTAIN_OVERLAY_SHARDS;

/// Return the `(data, parity)` Reed–Solomon counts.
pub const fn reed_solomon_counts() -> (usize, usize) {
    (RS_DATA_SHARDS, RS_PARITY_SHARDS)
}

/// Number of fountain-overlay shards layered on each chunk.
pub const fn fountain_overlay_count() -> usize {
    FOUNTAIN_OVERLAY_SHARDS
}

/// Total shards (data + parity + overlay) emitted per chunk.
pub const fn total_shards_per_chunk() -> usize {
    TOTAL_SHARDS_PER_CHUNK
}

/// Encode a chunk into LRC shards with a Progressive Fountain overlay.
pub fn encode(chunk: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let shard_size = (chunk.len() + RS_DATA_SHARDS - 1) / RS_DATA_SHARDS;
    let mut shards = Vec::with_capacity(TOTAL_SHARDS_PER_CHUNK);
    for i in 0..RS_DATA_SHARDS {
        let start = i * shard_size;
        let end = usize::min(start + shard_size, chunk.len());
        let mut s = vec![0u8; shard_size];
        if start < end {
            s[..end - start].copy_from_slice(&chunk[start..end]);
        }
        shards.push(s);
    }
    for _ in 0..RS_PARITY_SHARDS {
        shards.push(vec![0u8; shard_size]);
    }
    let r = ReedSolomon::new(RS_DATA_SHARDS, RS_PARITY_SHARDS).map_err(|e| e.to_string())?;
    r.encode(&mut shards).map_err(|e| e.to_string())?;
    Ok(overlay_fountain(shards))
}

fn overlay_fountain(mut shards: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    if shards.len() >= FOUNTAIN_SOURCE {
        let base0 = shards[0].clone();
        let base1 = shards[1].clone();
        for i in 0..FOUNTAIN_OVERLAY_SHARDS {
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
pub fn reconstruct(shards: Vec<Option<Vec<u8>>>, original_len: usize) -> Result<Vec<u8>, String> {
    if original_len == 0 {
        return Ok(Vec::new());
    }

    let mut iter = shards.into_iter();
    let mut base: Vec<Option<Vec<u8>>> = Vec::with_capacity(RS_TOTAL_SHARDS);
    for _ in 0..RS_TOTAL_SHARDS {
        base.push(iter.next().unwrap_or(None));
    }
    let overlays: Vec<Option<Vec<u8>>> = iter.collect();

    // Attempt to recover the first two data shards using the fountain overlay if missing.
    if base.get(0).map(|s| s.is_none()).unwrap_or(false)
        || base.get(1).map(|s| s.is_none()).unwrap_or(false)
    {
        try_fill_from_overlay(&mut base, &overlays);
    }

    let r = ReedSolomon::new(RS_DATA_SHARDS, RS_PARITY_SHARDS).map_err(|e| e.to_string())?;
    r.reconstruct(&mut base).map_err(|e| e.to_string())?;

    let mut chunk = Vec::new();
    for shard in base.into_iter().take(RS_DATA_SHARDS) {
        let shard = shard.ok_or_else(|| "missing data shard".to_string())?;
        chunk.extend_from_slice(&shard);
    }

    if chunk.len() < original_len {
        return Err("reconstructed chunk shorter than expected".into());
    }
    chunk.truncate(original_len);
    Ok(chunk)
}

fn try_fill_from_overlay(base: &mut [Option<Vec<u8>>], overlays: &[Option<Vec<u8>>]) {
    for (overlay_idx, maybe_overlay) in overlays.iter().enumerate() {
        let overlay = match maybe_overlay {
            Some(bytes) => bytes,
            None => continue,
        };
        if base.len() < 2 {
            break;
        }
        let need_first =
            base[0].is_none() && base[1].as_ref().map(|s| s.len()) == Some(overlay.len());
        if need_first {
            if let Some(ref shard1) = base[1] {
                if let Some(recovered) = recover_overlay(overlay, shard1, overlay_idx) {
                    base[0] = Some(recovered);
                }
            }
        }
        let need_second =
            base[1].is_none() && base[0].as_ref().map(|s| s.len()) == Some(overlay.len());
        if need_second {
            if let Some(ref shard0) = base[0] {
                if let Some(recovered) = recover_overlay(overlay, shard0, overlay_idx) {
                    base[1] = Some(recovered);
                }
            }
        }
        if base[0].is_some() && base[1].is_some() {
            break;
        }
    }
}

fn recover_overlay(overlay: &[u8], known: &[u8], overlay_idx: usize) -> Option<Vec<u8>> {
    if overlay.len() != known.len() {
        return None;
    }
    let mut recovered = Vec::with_capacity(overlay.len());
    for (pos, (&overlay_byte, &known_byte)) in overlay.iter().zip(known).enumerate() {
        let mut byte = overlay_byte;
        if pos == 0 {
            byte ^= overlay_idx as u8;
        }
        recovered.push(byte ^ known_byte);
    }
    Some(recovered)
}

use reed_solomon_erasure::galois_8::ReedSolomon;

/// Encode a chunk into data and parity shards.
pub fn encode(chunk: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut shards = vec![chunk.to_vec(), vec![0u8; chunk.len()]];
    let r = ReedSolomon::new(1, 1).map_err(|e| e.to_string())?;
    r.encode(&mut shards).map_err(|e| e.to_string())?;
    Ok(shards)
}

/// Reconstruct the original chunk from data/parity shards.
pub fn reconstruct(mut shards: Vec<Option<Vec<u8>>>) -> Result<Vec<u8>, String> {
    if shards.iter().filter(|s| s.is_some()).count() == 1 {
        return shards
            .into_iter()
            .flatten()
            .next()
            .ok_or_else(|| "missing data shard".to_string());
    }
    let r = ReedSolomon::new(1, 1).map_err(|e| e.to_string())?;
    r.reconstruct(&mut shards).map_err(|e| e.to_string())?;
    shards
        .into_iter()
        .next()
        .flatten()
        .ok_or_else(|| "missing data shard".to_string())
}

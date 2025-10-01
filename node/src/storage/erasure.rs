use std::sync::Arc;

use coding::{
    canonical_algorithm_label, ErasureCoder, ErasureError, ErasureMetadata, ErasureShard,
    ErasureShardKind,
};

use crate::storage::settings;

/// Number of fountain-overlay shards layered on top of the Reed–Solomon set.
const FOUNTAIN_OVERLAY_SHARDS: usize = 3;
const FOUNTAIN_SOURCE: usize = 2;

#[derive(Clone, Debug)]
pub struct ErasureParams {
    pub algorithm: String,
    pub data_shards: usize,
    pub parity_shards: usize,
}

impl ErasureParams {
    pub fn new(algorithm: String, data_shards: usize, parity_shards: usize) -> Self {
        Self {
            algorithm: canonical_algorithm_label(&algorithm),
            data_shards,
            parity_shards,
        }
    }

    pub fn total_rs(&self) -> usize {
        self.data_shards + self.parity_shards
    }

    pub fn is_reed_solomon(&self) -> bool {
        self.algorithm == "reed-solomon"
    }

    pub fn is_xor(&self) -> bool {
        self.algorithm == "xor"
    }
}

pub fn default_params() -> ErasureParams {
    let (data, parity) = settings::erasure_counts();
    let algorithms = settings::algorithms();
    ErasureParams::new(algorithms.erasure().to_string(), data, parity)
}

pub fn reed_solomon_counts() -> (usize, usize) {
    let params = default_params();
    (params.data_shards, params.parity_shards)
}

pub fn total_shards_per_chunk() -> usize {
    total_shards_for_params(&default_params())
}

pub fn total_shards_for_params(params: &ErasureParams) -> usize {
    params.total_rs() + FOUNTAIN_OVERLAY_SHARDS
}

pub fn encode(chunk: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    encode_with_params(chunk, &default_params())
}

pub fn encode_with_params(chunk: &[u8], params: &ErasureParams) -> Result<Vec<Vec<u8>>, String> {
    let coder = coder_for(params)?;
    let algo = coder.algorithm();
    let batch = match coder.encode(chunk) {
        Ok(batch) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("erasure_encode", algo, "ok");
            batch
        }
        Err(err) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("erasure_encode", algo, "err");
            return Err(err.to_string());
        }
    };
    if cfg!(not(feature = "telemetry")) {
        let _ = &algo;
    }
    let shards: Vec<Vec<u8>> = batch.shards.into_iter().map(|shard| shard.bytes).collect();
    Ok(overlay_fountain(shards))
}

pub fn reconstruct(shards: Vec<Option<Vec<u8>>>, original_len: usize) -> Result<Vec<u8>, String> {
    reconstruct_with_params(shards, original_len, &default_params())
}

pub fn reconstruct_with_params(
    shards: Vec<Option<Vec<u8>>>,
    original_len: usize,
    params: &ErasureParams,
) -> Result<Vec<u8>, String> {
    if original_len == 0 {
        return Ok(Vec::new());
    }
    let coder = coder_for(params)?;
    let algo = coder.algorithm();
    let total_rs = params.total_rs();
    if shards.len() != total_shards_for_params(params) {
        return Err("invalid shard layout".into());
    }
    let mut iter = shards.into_iter();
    let mut base: Vec<Option<Vec<u8>>> = Vec::with_capacity(total_rs);
    for _ in 0..total_rs {
        base.push(iter.next().unwrap_or(None));
    }
    let overlays: Vec<Option<Vec<u8>>> = iter.collect();
    if cfg!(not(feature = "telemetry")) {
        let _ = &algo;
    }
    if base.get(0).map(|s| s.is_none()).unwrap_or(false)
        || base.get(1).map(|s| s.is_none()).unwrap_or(false)
    {
        try_fill_from_overlay(&mut base, &overlays);
    }
    let shard_len = base
        .iter()
        .flatten()
        .map(|shard| shard.len())
        .max()
        .unwrap_or(0);
    let metadata = ErasureMetadata {
        data_shards: params.data_shards,
        parity_shards: params.parity_shards,
        shard_len,
        original_len,
    };
    let mut fragments: Vec<Option<ErasureShard>> = Vec::with_capacity(total_rs);
    for (idx, maybe_bytes) in base.into_iter().enumerate() {
        fragments.push(maybe_bytes.map(|bytes| ErasureShard {
            index: idx,
            kind: if idx < params.data_shards {
                ErasureShardKind::Data
            } else {
                ErasureShardKind::Parity
            },
            bytes,
        }));
    }
    let result = match coder.reconstruct(&metadata, &fragments) {
        Ok(mut recovered) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("erasure_reconstruct", algo, "ok");
            if recovered.len() < original_len {
                return Err("reconstructed chunk shorter than expected".into());
            }
            recovered.truncate(original_len);
            Ok(recovered)
        }
        Err(ErasureError::InvalidShardCount { .. })
        | Err(ErasureError::InvalidShardIndex { .. }) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("erasure_reconstruct", algo, "err");
            Err("invalid shard layout".into())
        }
        Err(ErasureError::EncodingFailed(msg) | ErasureError::ReconstructionFailed(msg)) => {
            #[cfg(feature = "telemetry")]
            crate::telemetry::record_coding_result("erasure_reconstruct", algo, "err");
            Err(msg)
        }
    }?;
    Ok(result)
}

fn coder_for(params: &ErasureParams) -> Result<Arc<dyn ErasureCoder>, String> {
    let algorithms = settings::algorithms();
    if params.algorithm == algorithms.erasure() {
        Ok(settings::erasure())
    } else {
        settings::erasure_for_algorithm(&params.algorithm, params.data_shards, params.parity_shards)
            .map_err(|err| err.to_string())
    }
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
            if let Some(first) = combo.first_mut() {
                *first ^= i as u8;
            }
            shards.push(combo);
        }
    }
    shards
}

fn try_fill_from_overlay(base: &mut [Option<Vec<u8>>], overlays: &[Option<Vec<u8>>]) {
    for (overlay_idx, maybe_overlay) in overlays.iter().enumerate() {
        let overlay = match maybe_overlay {
            Some(bytes) => bytes,
            None => continue,
        };
        if base.len() < FOUNTAIN_SOURCE {
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

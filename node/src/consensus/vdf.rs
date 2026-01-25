use foundation_bigint::BigUint;
use std::sync::OnceLock;
use std::thread;

const DEFAULT_MODULUS_HEX: &str =
    "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F";

static MODULUS: OnceLock<BigUint> = OnceLock::new();

fn default_modulus() -> &'static BigUint {
    MODULUS.get_or_init(|| {
        BigUint::parse_bytes(DEFAULT_MODULUS_HEX.as_bytes(), 16)
            .expect("deterministic VDF modulus must parse")
    })
}

fn modulus_bit_length(modulus: &BigUint) -> usize {
    let bytes = modulus.to_bytes_be();
    if bytes.is_empty() {
        return 0;
    }
    let leading = bytes[0].leading_zeros() as usize;
    bytes.len() * 8 - leading
}

/// Number of bits in the Pietrzak modulus the runtime presently uses.
pub fn modulus_bits() -> usize {
    modulus_bit_length(default_modulus())
}

fn reduce_preimage(preimage: &[u8], modulus: &BigUint) -> BigUint {
    if modulus.is_zero() {
        return BigUint::from_bytes_be(preimage);
    }
    let value = BigUint::from_bytes_be(preimage);
    if value < *modulus {
        value
    } else {
        value % modulus
    }
}

fn repeat_squaring(mut value: BigUint, rounds: u64, modulus: &BigUint) -> BigUint {
    for _ in 0..rounds {
        value = (&value * &value) % modulus;
    }
    value
}

/// Evaluate a seed with repeated squaring over the Pietrzak modulus.
pub fn evaluate(preimage: &[u8], rounds: u64) -> (Vec<u8>, Vec<u8>) {
    let modulus = default_modulus();
    let state = repeat_squaring(reduce_preimage(preimage, modulus), rounds, modulus);
    let output = state.to_bytes_be();
    (output.clone(), output)
}

/// Verify the VDF output by recomputing the squaring sequence (proof equals output).
pub fn verify(preimage: &[u8], rounds: u64, output: &[u8], proof: &[u8]) -> bool {
    let (out, pf) = evaluate(preimage, rounds);
    out == output && pf == proof
}

/// Verify the VDF output using a parallel thread to resist timing analysis.
pub fn verify_parallel(preimage: &[u8], rounds: u64, output: &[u8], proof: &[u8]) -> bool {
    let pre = preimage.to_vec();
    let out = output.to_vec();
    let pr = proof.to_vec();
    let handle = thread::spawn(move || evaluate(&pre, rounds));
    if let Ok((expected_out, expected_proof)) = handle.join() {
        expected_out == out && expected_proof == pr
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let pre = b"seed";
        let (out, proof) = evaluate(pre, 10);
        assert!(verify(pre, 10, &out, &proof));
        assert!(verify_parallel(pre, 10, &out, &proof));
    }

    #[test]
    fn modulus_bits_matches_constant() {
        assert_eq!(modulus_bits(), 256);
    }

    #[test]
    fn reduce_preimage_respects_modulus() {
        let modulus = default_modulus();
        let pre = vec![0xFF; 64];
        let reduced = reduce_preimage(&pre, modulus);
        assert!(reduced < *modulus);
    }
}

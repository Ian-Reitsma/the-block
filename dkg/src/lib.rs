#![deny(warnings)]

use std::collections::BTreeMap;
use threshold_crypto::{PublicKeySet, SecretKeySet, SecretKeyShare, Signature, SignatureShare};

/// Runs a basic DKG, returning the group public key set and participant secret shares.
pub fn run_dkg(participants: usize, threshold: usize) -> (PublicKeySet, Vec<SecretKeyShare>) {
    // `threshold` represents the minimum number of shares required to
    // reconstruct the key. The `threshold_crypto` crate expects the
    // polynomial degree, which is one less than the number of required
    // shares, so subtract one here to keep the API intuitive.
    let sk_set = SecretKeySet::random(threshold - 1, &mut rand::thread_rng());
    let pk_set = sk_set.public_keys();
    let mut shares = Vec::new();
    for i in 0..participants {
        shares.push(sk_set.secret_key_share(i));
    }
    (pk_set, shares)
}

/// Combines signature shares into a full signature.
pub fn combine(
    pk: &PublicKeySet,
    msg: &[u8],
    shares: &[(u64, SignatureShare)],
) -> Option<Signature> {
    let mut map = BTreeMap::new();
    for (id, share) in shares {
        map.insert(*id as usize, share.clone());
    }
    let sig = pk.combine_signatures(&map).ok()?;
    if pk.public_key().verify(&sig, msg) {
        Some(sig)
    } else {
        None
    }
}

use dkg::{combine, run_dkg, SignatureShare};

#[test]
fn key_refresh_and_dropout() {
    let (pk, shares) = run_dkg(5, 3);
    // sign message with first three shares
    let msg = b"hello";
    let mut sigs = Vec::new();
    for (i, share) in shares.iter().enumerate().take(3) {
        let sig_share: SignatureShare = share.sign(msg);
        sigs.push((i as u64, sig_share));
    }
    // combine should succeed with threshold shares
    let sig = combine(&pk, msg, &sigs).expect("combine");
    assert!(pk.public_key().verify(&sig, msg));

    // dropping below threshold fails
    let bad = &sigs[..2];
    assert!(combine(&pk, msg, bad).is_none());
}

#[test]
fn malicious_share_rejected() {
    let (pk, shares) = run_dkg(3, 2);
    let msg = b"data";
    let mut sigs = Vec::new();
    let s1 = shares[0].sign(msg);
    // Sign a different message to produce a share that is well-formed but invalid
    // for the target message.
    let s2 = shares[1].sign(b"tamper");
    sigs.push((0, s1));
    sigs.push((1, s2));
    // Combining with a mismatched share should fail verification.
    assert!(combine(&pk, msg, &sigs).is_none());
}

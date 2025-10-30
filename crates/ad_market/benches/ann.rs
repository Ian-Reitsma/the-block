use ad_market::badge::ann::{self, SoftIntentReceipt, WalletAnnIndexSnapshot};
use concurrency::Lazy;
use crypto_suite::hashing::blake3;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use testkit::tb_bench;

struct AnnFixture {
    snapshot: WalletAnnIndexSnapshot,
    receipt: SoftIntentReceipt,
    badges: Vec<String>,
}

static ANN_FIXTURES: Lazy<Vec<AnnFixture>> = Lazy::new(|| {
    let mut rng = StdRng::seed_from_u64(0xANN5EED_u64);
    let mut fixtures = Vec::new();
    let bucket_targets = [128usize, 512, 2_048, 8_192, 32_768];

    for &bucket_count in &bucket_targets {
        let mut fingerprint = [0u8; blake3::OUT_LEN];
        rng.fill_bytes(&mut fingerprint);

        let badge_count = match bucket_count {
            128 => 24,
            512 => 96,
            2_048 => 320,
            8_192 => 640,
            _ => 1_024,
        };
        let badges = (0..badge_count)
            .map(|idx| format!("badge-{bucket_count}-{idx}"))
            .collect::<Vec<_>>();
        let query_hash = ann::hash_badges(&badges);

        let mut buckets = Vec::with_capacity(bucket_count);
        buckets.push(query_hash);
        while buckets.len() < bucket_count {
            let mut bucket = [0u8; blake3::OUT_LEN];
            rng.fill_bytes(&mut bucket);
            buckets.push(bucket);
        }

        let dimensions = badge_count.min(u16::MAX as usize) as u16;
        let snapshot = WalletAnnIndexSnapshot::new(fingerprint, buckets, dimensions);
        let snapshot = if bucket_count >= 2_048 {
            let mut salt = vec![0u8; blake3::OUT_LEN];
            rng.fill_bytes(&mut salt);
            snapshot.with_entropy_salt(salt)
        } else {
            snapshot
        };
        let wallet_entropy = if bucket_count >= 2_048 {
            let mut entropy = vec![0u8; blake3::OUT_LEN];
            rng.fill_bytes(&mut entropy);
            Some(entropy)
        } else {
            None
        };
        let receipt = match wallet_entropy {
            Some(ref entropy) => {
                ann::build_proof_with_entropy(&snapshot, &badges, Some(entropy.as_slice()))
            }
            None => ann::build_proof(&snapshot, &badges),
        }
        .expect("soft intent receipt");

        fixtures.push(AnnFixture {
            snapshot,
            receipt,
            badges,
        });
    }

    fixtures
});

tb_bench!(ann_soft_intent_verification, {
    for fixture in ANN_FIXTURES.iter() {
        assert!(ann::verify_receipt(
            &fixture.snapshot,
            &fixture.receipt,
            &fixture.badges
        ));
    }
});

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::BTreeSet;

#[test]
fn fill_produces_deterministic_bytes() {
    let mut rng_a = StdRng::seed_from_u64(0xfeed_cafe);
    let mut buf_a = [0u8; 32];
    rng_a.fill(&mut buf_a);
    assert_ne!(buf_a, [0u8; 32]);

    let mut rng_b = StdRng::seed_from_u64(0xfeed_cafe);
    let mut buf_b = [0u8; 32];
    rng_b.fill(&mut buf_b);
    assert_eq!(buf_a, buf_b, "identical seeds must yield identical output");
}

#[test]
fn choose_and_choose_mut_align_for_same_seed() {
    let values = vec![10, 20, 30, 40, 50];

    let mut rng_read = StdRng::seed_from_u64(0x1234);
    let chosen_read = *values
        .choose(&mut rng_read)
        .expect("non-empty slice yields a value");

    let mut rng_write = StdRng::seed_from_u64(0x1234);
    let mut mutable = values.clone();
    {
        let chosen_write = mutable
            .choose_mut(&mut rng_write)
            .expect("non-empty slice yields a value");
        assert_eq!(*chosen_write, chosen_read);
        *chosen_write = 99;
    }

    assert!(mutable.contains(&99));
}

#[test]
fn choose_multiple_draws_unique_values() {
    let data = vec![1, 2, 3, 4, 5, 6];
    let mut rng = StdRng::seed_from_u64(0x7777);
    let picks = data.choose_multiple(&mut rng, 4);
    assert_eq!(picks.len(), 4);
    let mut seen = BTreeSet::new();
    for pick in picks {
        assert!(data.contains(pick));
        assert!(seen.insert(*pick));
    }
}

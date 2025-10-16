use rand::rngs::StdRng;
use rand::Rng;
use std::collections::HashSet;

#[test]
fn gen_range_handles_large_u64_domains() {
    let mut rng = StdRng::seed_from_u64(0xface_cafe);
    for _ in 0..128 {
        let value = rng.gen_range(u64::MAX - 1024..u64::MAX);
        assert!(value >= u64::MAX - 1024);
        assert!(value < u64::MAX);
    }

    let mut seen = HashSet::new();
    for _ in 0..512 {
        let value = rng.gen_range(u64::MAX - 5..=u64::MAX);
        assert!(value >= u64::MAX - 5);
        assert!(value <= u64::MAX);
        seen.insert(value);
    }
    assert!(seen.len() > 1, "rng should explore the inclusive tail");
}

#[test]
fn gen_range_handles_full_i64_span() {
    let mut rng = StdRng::seed_from_u64(0xdead_beef);
    for _ in 0..256 {
        let value = rng.gen_range(i64::MIN..=i64::MAX);
        assert!(value >= i64::MIN);
        assert!(value <= i64::MAX);
    }
}

#[test]
fn gen_range_respects_usize_full_range() {
    let mut rng = StdRng::seed_from_u64(0x0123_4567_89ab_cdef);
    for _ in 0..64 {
        let value = rng.gen_range(0..=usize::MAX);
        assert!(value <= usize::MAX);
    }
}

#[test]
fn gen_range_supports_u32_edges() {
    let mut rng = StdRng::seed_from_u64(0xfeed_face_cafe_f00d);

    let tail = rng.gen_range(u32::MAX - 1..u32::MAX);
    assert_eq!(
        tail,
        u32::MAX - 1,
        "exclusive tail should never reach the end bound"
    );

    let singleton = rng.gen_range(u32::MAX..=u32::MAX);
    assert_eq!(
        singleton,
        u32::MAX,
        "inclusive singleton range should return the bound"
    );

    let span = rng.gen_range(u32::MAX - 10..=u32::MAX - 5);
    assert!(
        (u32::MAX - 10..=u32::MAX - 5).contains(&span),
        "value should stay inside the inclusive span"
    );
}

#[test]
fn gen_range_supports_i32_edges() {
    let mut rng = StdRng::seed_from_u64(0x0ddf_aced_dead_beef);

    let tail = rng.gen_range(i32::MIN..i32::MIN + 1);
    assert_eq!(
        tail,
        i32::MIN,
        "exclusive signed range should return the lower bound when span is 1"
    );

    let singleton = rng.gen_range(i32::MAX..=i32::MAX);
    assert_eq!(
        singleton,
        i32::MAX,
        "inclusive singleton range should return the bound"
    );

    let span = rng.gen_range(i32::MIN + 10..=i32::MIN + 20);
    assert!(
        (i32::MIN + 10..=i32::MIN + 20).contains(&span),
        "value should stay inside the inclusive signed span"
    );
}

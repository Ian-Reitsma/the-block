//! Workspace-local stand-in for the upstream `rand` crate.
//!
//! Only the subset required by the codebase is implemented. Algorithms are
//! intentionally lightweight until dedicated first-party primitives land.

use std::cell::RefCell;
use std::ops::{Range, RangeInclusive};
use std::time::{SystemTime, UNIX_EPOCH};

pub use rand_core::{CryptoRng, Error, ErrorKind, OsRng, RngCore};

fn seed_from_entropy() -> [u8; 32] {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let pid = std::process::id() as u128;

    let mut seed = [0u8; 32];
    seed[..16].copy_from_slice(&now.to_le_bytes());
    seed[16..].copy_from_slice(&(now ^ pid).to_le_bytes());
    seed
}

thread_local! {
    static THREAD_RNG: RefCell<StdRng> = RefCell::new(StdRng::from_seed(seed_from_entropy()));
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadRng;

pub fn thread_rng() -> ThreadRng {
    ThreadRng
}

impl ThreadRng {
    fn with_mut<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce(&mut StdRng) -> T,
    {
        THREAD_RNG.with(|cell| {
            let mut rng = cell.borrow_mut();
            f(&mut rng)
        })
    }
}

impl RngCore for ThreadRng {
    fn next_u32(&mut self) -> u32 {
        self.with_mut(|rng| rng.next_u32())
    }

    fn next_u64(&mut self) -> u64 {
        self.with_mut(|rng| rng.next_u64())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.with_mut(|rng| rng.fill_bytes(dest))
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.with_mut(|rng| rng.try_fill_bytes(dest))
    }
}

impl CryptoRng for ThreadRng {}

#[derive(Clone, Debug)]
pub struct StdRng {
    state: u128,
}

impl StdRng {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let mut acc = 0u128;
        for chunk in seed.chunks(8) {
            let mut buf = [0u8; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            acc ^= u64::from_le_bytes(buf) as u128;
            acc = acc.rotate_left(11) ^ 0x9e37_79b9_7f4a_7c15;
        }
        Self { state: acc }
    }

    pub fn from_rng<R: RngCore>(mut rng: R) -> Result<Self, Error> {
        let mut seed = [0u8; 32];
        rng.try_fill_bytes(&mut seed)?;
        Ok(Self::from_seed(seed))
    }

    pub fn seed_from_u64(seed: u64) -> Self {
        let mut buf = [0u8; 32];
        buf[..8].copy_from_slice(&seed.to_le_bytes());
        Self::from_seed(buf)
    }
}

impl RngCore for StdRng {
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() & 0xffff_ffff) as u32
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 7;
        x ^= x >> 9;
        x ^= x << 13;
        self.state = x;
        (x ^ (x >> 32)) as u64
    }
}

impl CryptoRng for StdRng {}

pub mod rngs {
    pub use crate::{OsRng, StdRng};
}

pub trait SeedableRng: Sized {
    type Seed;

    fn from_seed(seed: Self::Seed) -> Self;
    fn from_rng<R: RngCore>(rng: R) -> Result<Self, Error>;
}

impl SeedableRng for StdRng {
    type Seed = [u8; 32];

    fn from_seed(seed: Self::Seed) -> Self {
        StdRng::from_seed(seed)
    }

    fn from_rng<R: RngCore>(rng: R) -> Result<Self, Error> {
        StdRng::from_rng(rng)
    }
}

pub trait Rng: RngCore + Sized {
    fn gen<T>(&mut self) -> T
    where
        SampleUniform<T>: UniformSampler<T>,
    {
        <SampleUniform<T> as UniformSampler<T>>::sample(self)
    }

    fn gen_range<T, R>(&mut self, range: R) -> T
    where
        R: SampleRange<T>,
    {
        range.sample(self)
    }

    fn gen_bool(&mut self, p: f64) -> bool {
        let threshold = if p.is_nan() { 0.0 } else { p.clamp(0.0, 1.0) };
        self.gen::<f64>() < threshold
    }

    fn fill<T: AsMut<[u8]>>(&mut self, mut dest: T) {
        self.fill_bytes(dest.as_mut());
    }

    fn try_fill<T: AsMut<[u8]>>(&mut self, mut dest: T) -> Result<(), Error> {
        self.try_fill_bytes(dest.as_mut())
    }
}

impl<T: RngCore> Rng for T {}

pub trait UniformSampler<T> {
    fn sample<R: RngCore + ?Sized>(rng: &mut R) -> T;
}

pub struct SampleUniform<T>(std::marker::PhantomData<T>);

impl UniformSampler<f64> for SampleUniform<f64> {
    fn sample<R: RngCore + ?Sized>(rng: &mut R) -> f64 {
        (rng.next_u64() as f64) / (u64::MAX as f64)
    }
}

impl UniformSampler<u64> for SampleUniform<u64> {
    fn sample<R: RngCore + ?Sized>(rng: &mut R) -> u64 {
        rng.next_u64()
    }
}

impl UniformSampler<u32> for SampleUniform<u32> {
    fn sample<R: RngCore + ?Sized>(rng: &mut R) -> u32 {
        rng.next_u32()
    }
}

impl UniformSampler<usize> for SampleUniform<usize> {
    fn sample<R: RngCore + ?Sized>(rng: &mut R) -> usize {
        rng.next_u64() as usize
    }
}

impl UniformSampler<bool> for SampleUniform<bool> {
    fn sample<R: RngCore + ?Sized>(rng: &mut R) -> bool {
        rng.next_u64() & 1 == 1
    }
}

fn uniform_sample_u64<R: RngCore + ?Sized>(rng: &mut R, span: u64) -> u64 {
    assert!(span > 0, "span must be > 0");
    let limit = u64::MAX - (u64::MAX % span);
    loop {
        let value = rng.next_u64();
        if value < limit {
            return value % span;
        }
    }
}

fn uniform_sample_u128<R: RngCore + ?Sized>(rng: &mut R, upper: u128) -> u128 {
    assert!(upper > 0, "upper bound must be > 0");
    if upper <= u64::MAX as u128 {
        return uniform_sample_u64(rng, upper as u64) as u128;
    }
    let limit = u128::MAX - (u128::MAX % upper);
    loop {
        let hi = rng.next_u64() as u128;
        let lo = rng.next_u64() as u128;
        let value = (hi << 64) | lo;
        if value < limit {
            return value % upper;
        }
    }
}

fn sample_range_exclusive_u64<R: RngCore + ?Sized>(rng: &mut R, start: u64, end: u64) -> u64 {
    assert!(start < end, "invalid range: start >= end");
    let span = end - start;
    start + uniform_sample_u64(rng, span)
}

fn sample_range_inclusive_u64<R: RngCore + ?Sized>(rng: &mut R, start: u64, end: u64) -> u64 {
    if start >= end {
        return start;
    }
    let span = end - start;
    if span == u64::MAX {
        return rng.next_u64();
    }
    start + uniform_sample_u64(rng, span + 1)
}

fn sample_range_exclusive_usize<R: RngCore + ?Sized>(
    rng: &mut R,
    start: usize,
    end: usize,
) -> usize {
    assert!(start < end, "invalid range: start >= end");
    let span = end - start;
    if span == 0 {
        return start;
    }
    if span as u128 > u64::MAX as u128 {
        let offset = uniform_sample_u128(rng, span as u128);
        start + offset as usize
    } else {
        start + uniform_sample_u64(rng, span as u64) as usize
    }
}

fn sample_range_inclusive_usize<R: RngCore + ?Sized>(
    rng: &mut R,
    start: usize,
    end: usize,
) -> usize {
    if start >= end {
        return start;
    }
    let span = end - start;
    if span == usize::MAX {
        // Covers the full domain; rely on the underlying RNG width.
        return rng.next_u64() as usize;
    }
    if span as u128 >= u64::MAX as u128 {
        let offset = uniform_sample_u128(rng, span as u128 + 1);
        start + offset as usize
    } else {
        start + uniform_sample_u64(rng, span as u64 + 1) as usize
    }
}

fn sample_range_exclusive_i64<R: RngCore + ?Sized>(rng: &mut R, start: i64, end: i64) -> i64 {
    assert!(start < end, "invalid range: start >= end");
    let span = (end as i128) - (start as i128);
    let offset = uniform_sample_u128(rng, span as u128) as i128;
    (start as i128 + offset) as i64
}

fn sample_range_inclusive_i64<R: RngCore + ?Sized>(rng: &mut R, start: i64, end: i64) -> i64 {
    if end <= start {
        return start;
    }
    let span = (end as i128) - (start as i128);
    if span as u128 == u64::MAX as u128 {
        // Entire domain; bias-correct by translating the raw u64 into i64 space.
        let raw = rng.next_u64();
        let signed = (raw as i128) + i64::MIN as i128;
        return signed as i64;
    }
    let offset = uniform_sample_u128(rng, span as u128 + 1) as i128;
    (start as i128 + offset) as i64
}

pub trait SampleRange<T> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> T;
}

impl SampleRange<u64> for Range<u64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> u64 {
        sample_range_exclusive_u64(rng, self.start, self.end)
    }
}

impl SampleRange<u64> for RangeInclusive<u64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> u64 {
        let (start, end) = self.into_inner();
        sample_range_inclusive_u64(rng, start, end)
    }
}

impl SampleRange<u32> for Range<u32> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> u32 {
        sample_range_exclusive_u64(rng, self.start as u64, self.end as u64) as u32
    }
}

impl SampleRange<u32> for RangeInclusive<u32> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> u32 {
        let (start, end) = self.into_inner();
        sample_range_inclusive_u64(rng, start as u64, end as u64) as u32
    }
}

impl SampleRange<usize> for Range<usize> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> usize {
        sample_range_exclusive_usize(rng, self.start, self.end)
    }
}

impl SampleRange<usize> for RangeInclusive<usize> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> usize {
        let (start, end) = self.into_inner();
        sample_range_inclusive_usize(rng, start, end)
    }
}

impl SampleRange<i64> for Range<i64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> i64 {
        sample_range_exclusive_i64(rng, self.start, self.end)
    }
}

impl SampleRange<i64> for RangeInclusive<i64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> i64 {
        let (start, end) = self.into_inner();
        sample_range_inclusive_i64(rng, start, end)
    }
}

impl SampleRange<i32> for Range<i32> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> i32 {
        sample_range_exclusive_i64(rng, self.start as i64, self.end as i64) as i32
    }
}

impl SampleRange<i32> for RangeInclusive<i32> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> i32 {
        let (start, end) = self.into_inner();
        sample_range_inclusive_i64(rng, start as i64, end as i64) as i32
    }
}

impl SampleRange<f64> for Range<f64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> f64 {
        let span = self.end - self.start;
        self.start + <SampleUniform<f64> as UniformSampler<f64>>::sample(rng) * span
    }
}

impl SampleRange<f64> for RangeInclusive<f64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> f64 {
        let (start, end) = self.into_inner();
        if (end - start).abs() < f64::EPSILON {
            return start;
        }
        start + <SampleUniform<f64> as UniformSampler<f64>>::sample(rng) * (end - start)
    }
}

pub mod seq {
    use super::{uniform_sample_u64, RngCore};
    use std::vec::Vec;

    pub trait SliceRandom<T> {
        fn shuffle<R: RngCore>(&mut self, rng: &mut R);

        fn choose<'a, R: RngCore>(&'a self, rng: &mut R) -> Option<&'a T>;

        fn choose_mut<'a, R: RngCore>(&'a mut self, rng: &mut R) -> Option<&'a mut T>;

        fn choose_multiple<'a, R: RngCore>(&'a self, rng: &mut R, amount: usize) -> Vec<&'a T>;
    }

    fn random_index(len: usize, rng: &mut impl RngCore) -> usize {
        if len <= 1 {
            return 0;
        }
        let span = len as u64;
        uniform_sample_u64(rng, span) as usize
    }

    impl<T> SliceRandom<T> for [T] {
        fn shuffle<R: RngCore>(&mut self, rng: &mut R) {
            let len = self.len();
            if len <= 1 {
                return;
            }
            for i in (1..len).rev() {
                let j = random_index(i + 1, rng);
                self.swap(i, j);
            }
        }

        fn choose<'a, R: RngCore>(&'a self, rng: &mut R) -> Option<&'a T> {
            if self.is_empty() {
                return None;
            }
            let idx = random_index(self.len(), rng);
            self.get(idx)
        }

        fn choose_mut<'a, R: RngCore>(&'a mut self, rng: &mut R) -> Option<&'a mut T> {
            if self.is_empty() {
                return None;
            }
            let idx = random_index(self.len(), rng);
            self.get_mut(idx)
        }

        fn choose_multiple<'a, R: RngCore>(&'a self, rng: &mut R, amount: usize) -> Vec<&'a T> {
            if amount == 0 || self.is_empty() {
                return Vec::new();
            }
            let take = amount.min(self.len());
            let mut indices: Vec<usize> = (0..self.len()).collect();
            indices.shuffle(rng);
            indices.truncate(take);
            indices.sort_unstable();
            indices
                .into_iter()
                .filter_map(|idx| self.get(idx))
                .collect()
        }
    }
}

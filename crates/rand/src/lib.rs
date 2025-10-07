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

pub trait SampleRange<T> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> T;
}

impl SampleRange<u64> for Range<u64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> u64 {
        let span = self.end - self.start;
        self.start + (rng.next_u64() % span)
    }
}

impl SampleRange<u64> for RangeInclusive<u64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> u64 {
        let (start, end) = self.into_inner();
        if end <= start {
            return start;
        }
        start + (rng.next_u64() % (end - start + 1))
    }
}

impl SampleRange<usize> for Range<usize> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> usize {
        let span = self.end - self.start;
        self.start + (rng.next_u64() as usize % span)
    }
}

impl SampleRange<usize> for RangeInclusive<usize> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> usize {
        let (start, end) = self.into_inner();
        if end <= start {
            return start;
        }
        start + (rng.next_u64() as usize % (end - start + 1))
    }
}

impl SampleRange<i64> for Range<i64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> i64 {
        let span = self.end - self.start;
        self.start + (rng.next_u64() as i64 % span)
    }
}

impl SampleRange<i64> for RangeInclusive<i64> {
    fn sample<R: RngCore + ?Sized>(self, rng: &mut R) -> i64 {
        let (start, end) = self.into_inner();
        if end <= start {
            return start;
        }
        start + (rng.next_u64() as i64 % (end - start + 1))
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
    use super::RngCore;

    pub trait SliceRandom<T> {
        fn shuffle<R: RngCore>(&mut self, rng: &mut R);
    }

    impl<T> SliceRandom<T> for [T] {
        fn shuffle<R: RngCore>(&mut self, rng: &mut R) {
            let len = self.len();
            if len <= 1 {
                return;
            }
            for i in (1..len).rev() {
                let j = (rng.next_u64() as usize) % (i + 1);
                self.swap(i, j);
            }
        }
    }
}

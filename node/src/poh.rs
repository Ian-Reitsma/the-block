use crypto_suite::hashing::blake3::Hasher;

/// Proof-of-history tick generator using a sequential hash chain.
#[derive(Clone)]
pub struct Poh {
    hash: [u8; 32],
    ticks: u64,
}

impl Poh {
    /// Create a new PoH instance seeded with `seed`.
    #[must_use]
    pub fn new(seed: &[u8]) -> Self {
        let hash = hash_step(seed);
        Self { hash, ticks: 0 }
    }

    /// Generate the next tick hash.
    #[must_use]
    pub fn tick(&mut self) -> [u8; 32] {
        self.hash = hash_step(&self.hash);
        self.ticks += 1;
        self.hash
    }

    /// Record arbitrary data into the PoH sequence.
    #[must_use]
    pub fn record(&mut self, data: &[u8]) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&self.hash);
        h.update(data);
        self.hash = finalize_hash(h);
        self.ticks += 1;
        self.hash
    }

    /// Current hash at this point in the sequence.
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }

    /// Number of ticks produced so far.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }
}

fn finalize_hash(h: Hasher) -> [u8; 32] {
    let out = h.finalize();
    *out.as_bytes()
}

fn hash_step(data: &[u8]) -> [u8; 32] {
    #[cfg(feature = "gpu")]
    {
        // When GPU feature is enabled, leverage the GPU hash workload used for compute jobs.
        // This preserves deterministic results while allowing GPU offload where available.
        return crate::compute_market::workloads::gpu::run(data).output;
    }
    #[cfg(not(feature = "gpu"))]
    {
        let mut h = Hasher::new();
        h.update(data);
        finalize_hash(h)
    }
}

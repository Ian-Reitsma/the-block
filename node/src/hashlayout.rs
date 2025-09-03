use blake3::Hasher;

pub trait HashEncoder {
    fn encode(&self, h: &mut Hasher);
}

pub struct BlockEncoder<'a> {
    pub index: u64,
    pub prev: &'a str,
    pub timestamp: u64,
    pub nonce: u64,
    pub difficulty: u64,
    pub coin_c: u64,
    pub coin_i: u64,
    pub storage_sub: u64,
    pub read_sub: u64,
    pub compute_sub: u64,
    pub read_root: [u8;32],
    pub fee_checksum: &'a str,
    pub state_root: &'a str,
    pub tx_ids: &'a [&'a [u8]],
    pub l2_roots: &'a [[u8; 32]],
    pub l2_sizes: &'a [u32],
    pub vdf_commit: [u8;32],
    pub vdf_output: [u8;32],
    pub vdf_proof: &'a [u8],
}

impl<'a> HashEncoder for BlockEncoder<'a> {
    fn encode(&self, h: &mut Hasher) {
        h.update(&self.index.to_le_bytes());
        h.update(self.prev.as_bytes());
        h.update(&self.timestamp.to_le_bytes());
        h.update(&self.nonce.to_le_bytes());
        h.update(&self.difficulty.to_le_bytes());
        h.update(&self.coin_c.to_le_bytes());
        h.update(&self.coin_i.to_le_bytes());
        h.update(&self.storage_sub.to_le_bytes());
        h.update(&self.read_sub.to_le_bytes());
        h.update(&self.compute_sub.to_le_bytes());
        h.update(&self.read_root);
        h.update(self.fee_checksum.as_bytes());
        h.update(self.state_root.as_bytes());
        for r in self.l2_roots {
            h.update(r);
        }
        for s in self.l2_sizes {
            h.update(&s.to_le_bytes());
        }
        h.update(&self.vdf_commit);
        h.update(&self.vdf_output);
        h.update(&(self.vdf_proof.len() as u32).to_le_bytes());
        h.update(self.vdf_proof);
        for id in self.tx_ids {
            h.update(id);
        }
    }
}

impl<'a> BlockEncoder<'a> {
    pub fn hash(&self) -> String {
        let mut h = Hasher::new();
        self.encode(&mut h);
        h.finalize().to_hex().to_string()
    }

    /// Const variant used for compile-time genesis hash calculation.
    pub const fn const_hash(&self) -> &'static str {
        GENESIS_HASH
    }
}

pub const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
pub const GENESIS_HASH: &str = include_str!(concat!(env!("OUT_DIR"), "/genesis_hash.txt"));

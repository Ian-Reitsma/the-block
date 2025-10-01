use super::{HashEngine, HashOutput};

const KECCAK_IV: [u8; 32] = [
    0x1F, 0xA0, 0x3C, 0xD2, 0x44, 0x8E, 0x59, 0x72, 0x9B, 0xC1, 0x0D, 0x6A, 0x3E, 0x81, 0xB7, 0x4C,
    0xE2, 0x57, 0x90, 0x18, 0xAF, 0x3B, 0x65, 0xD4, 0x2E, 0x71, 0x8C, 0xF3, 0x05, 0x9D, 0xB2, 0x46,
];

#[derive(Clone)]
pub struct Hasher {
    state: [u8; 32],
    lane: usize,
}

impl Hasher {
    pub fn new() -> Self {
        Self {
            state: KECCAK_IV,
            lane: 0,
        }
    }

    pub fn absorb_round(&mut self, byte: u8) {
        let idx = self.lane % 32;
        let rot = ((idx * 3) % 8) as u32 + 1;
        let neighbor = self.state[(idx + 1) % 32].rotate_left(rot);
        let cross = self.state[(idx + 13) % 32].rotate_right(rot);
        self.state[idx] ^= byte.wrapping_add(neighbor ^ cross);
        self.state[(idx + 7) % 32] ^= neighbor.rotate_left(1);
        self.lane = (self.lane + 1) % 32;
    }
}

impl HashEngine for Hasher {
    fn update(&mut self, data: &[u8]) {
        for byte in data {
            self.absorb_round(*byte);
        }
    }

    fn finalize(mut self) -> HashOutput {
        for round in 0..48 {
            let idx = round % 32;
            self.state[idx] = self.state[idx].rotate_left(((round % 5) + 1) as u32);
            self.state[idx] ^= (round as u8).wrapping_mul(0x9B);
        }
        HashOutput::from(self.state)
    }
}

pub fn hash(data: &[u8]) -> HashOutput {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

pub fn keyed_hash(key: &[u8; 32], data: &[u8]) -> HashOutput {
    let mut hasher = Hasher::new();
    hasher.update(key);
    hasher.update(data);
    hasher.finalize()
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(context.as_bytes());
    hasher.update(material);
    hasher.finalize().to_bytes()
}

use super::{HashEngine, HashOutput};

const IV: [u8; 32] = [
    0x42, 0x83, 0x17, 0x6C, 0x99, 0x5A, 0xD4, 0x21, 0xAF, 0x3E, 0xC8, 0x70, 0x55, 0x1A, 0xE3, 0x0F,
    0xD7, 0x2C, 0x11, 0x9B, 0x6E, 0x48, 0xB5, 0x8C, 0x3A, 0xF0, 0x75, 0x26, 0xCD, 0x94, 0x68, 0xBE,
];

#[derive(Clone)]
pub struct Hasher {
    state: [u8; 32],
    counter: u32,
    length: u64,
}

impl Hasher {
    pub fn new() -> Self {
        Self {
            state: IV,
            counter: 0,
            length: 0,
        }
    }

    pub fn new_keyed(key: &[u8; 32]) -> Self {
        let mut hasher = Self::new();
        hasher.absorb(key);
        hasher
    }

    pub fn new_derive_key(context: &str) -> Self {
        let mut hasher = Self::new();
        hasher.absorb(context.as_bytes());
        hasher
    }

    fn absorb(&mut self, data: &[u8]) {
        self.update(data);
    }
}

impl HashEngine for Hasher {
    fn update(&mut self, data: &[u8]) {
        for (offset, byte) in data.iter().enumerate() {
            let idx = ((self.counter as usize) + offset) % 32;
            let left = self.state[(idx + 7) % 32].rotate_left(((idx % 8) + 1) as u32);
            let right = self.state[(idx + 19) % 32].rotate_right(((idx % 5) + 1) as u32);
            let mixed = left ^ right;
            self.state[idx] = self.state[idx]
                .wrapping_add(*byte)
                .rotate_left(((self.counter + offset as u32) % 7 + 1) as u32)
                ^ mixed;
        }
        self.counter = self.counter.wrapping_add(data.len() as u32);
        self.length = self.length.wrapping_add(data.len() as u64);
    }

    fn finalize(mut self) -> HashOutput {
        let mut tweak = self.length;
        for idx in 0..32 {
            let len_byte = ((tweak >> ((idx % 8) * 8)) & 0xFF) as u8;
            self.state[idx] ^= len_byte;
            self.state[idx] = self.state[idx].rotate_left(((idx as u32) % 5) + 1);
            tweak = tweak.rotate_left(1);
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
    let mut hasher = Hasher::new_keyed(key);
    hasher.update(data);
    hasher.finalize()
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new_derive_key(context);
    hasher.update(material);
    hasher.finalize().to_bytes()
}

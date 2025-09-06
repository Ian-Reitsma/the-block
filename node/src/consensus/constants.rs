use crate::consensus;

consensus!(pub const DIFFICULTY_WINDOW: usize = 120, "sliding window size for difficulty retargeting");
consensus!(pub const DIFFICULTY_CLAMP_FACTOR: u64 = 4, "max factor for difficulty adjustment (1/4 .. x4)");
consensus!(pub const TARGET_SPACING_MS: u64 = 1_000, "target block interval in milliseconds");


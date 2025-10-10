//! First-party crypto primitives.
//!
//! Provides OS randomness, hashing, encoders, and numeric helpers backed by
//! in-house implementations shared across the workspace.

/// Deterministic and entropy-backed randomness utilities.
pub mod rng {
    use std::fmt;

    use sys::{error::SysError, random};

    /// Error returned when an RNG operation is unavailable.
    #[derive(Debug)]
    pub struct RngError {
        context: &'static str,
        source: Option<SysError>,
    }

    impl RngError {
        pub const fn unsupported(context: &'static str) -> Self {
            Self {
                context,
                source: None,
            }
        }

        pub fn from_sys(context: &'static str, source: SysError) -> Self {
            Self {
                context,
                source: Some(source),
            }
        }
    }

    impl fmt::Display for RngError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match &self.source {
                Some(source) => write!(f, "{context}: {source}", context = self.context),
                None => write!(f, "{context}", context = self.context),
            }
        }
    }

    impl std::error::Error for RngError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_ref()
                .map(|err| err as &(dyn std::error::Error + 'static))
        }
    }

    /// OS-backed secure RNG.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct OsRng;

    impl OsRng {
        /// Fill the destination buffer with secure random bytes.
        pub fn fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RngError> {
            random::fill_bytes(dest).map_err(|err| RngError::from_sys("os rng fill", err))
        }

        /// Generate the next 64 bits of randomness.
        pub fn next_u64(&mut self) -> Result<u64, RngError> {
            random::fill_u64().map_err(|err| RngError::from_sys("os rng next_u64", err))
        }
    }

    /// Deterministic RNG used for reproducible testing.
    #[derive(Debug, Clone, Copy)]
    pub struct DeterministicRng {
        seed: u64,
    }

    impl DeterministicRng {
        /// Construct a new deterministic RNG from a seed value.
        pub const fn from_seed(seed: u64) -> Self {
            Self { seed }
        }

        /// Produce the next 64-bit value from the deterministic stream.
        pub fn next_u64(&mut self) -> u64 {
            let mut x = self.seed;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.seed = x;
            x
        }

        /// Fill the provided buffer with pseudorandom bytes.
        pub fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let value = self.next_u64().to_le_bytes();
                let len = chunk.len();
                chunk.copy_from_slice(&value[..len]);
            }
        }
    }
}

/// Hashing helpers implemented in-house.
pub mod hash {
    pub use blake3_impl::{
        Hash as Blake3Hash, Hasher as Blake3Hasher, HexOutput as Blake3HexOutput,
        KEY_LEN as BLAKE3_KEY_LEN, OUT_LEN as BLAKE3_OUT_LEN,
    };

    /// Compute the BLAKE3 hash of the provided data and return the raw digest bytes.
    pub fn blake3(data: &[u8]) -> [u8; BLAKE3_OUT_LEN] {
        blake3_impl::hash(data).to_bytes()
    }

    /// Compute the BLAKE3 hash of the provided data and return the strong hash wrapper.
    pub fn blake3_hash(data: &[u8]) -> Blake3Hash {
        blake3_impl::hash(data)
    }

    /// Compute the keyed BLAKE3 hash using the provided secret key.
    pub fn blake3_keyed(key: &[u8; BLAKE3_KEY_LEN], data: &[u8]) -> Blake3Hash {
        blake3_impl::keyed_hash(key, data)
    }

    /// Derive a key using the BLAKE3 derive-key mode.
    pub fn blake3_derive_key(context: &str, material: &[u8]) -> [u8; BLAKE3_KEY_LEN] {
        blake3_impl::derive_key(context, material)
    }

    /// Compute an extendable-output BLAKE3 hash into the provided buffer.
    pub fn blake3_xof(data: &[u8], out: &mut [u8]) {
        blake3_impl::xof(data, out);
    }

    /// Compute the SHA-256 digest of the provided data.
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        sha256_impl::digest(data)
    }

    mod blake3_impl {
        use core::cmp::min;
        use core::convert::TryInto;
        use core::fmt::{self, Write as _};
        use std::string::String;

        pub const OUT_LEN: usize = 32;
        pub const KEY_LEN: usize = 32;
        const BLOCK_LEN: usize = 64;
        const CHUNK_LEN: usize = 1024;

        const CHUNK_START: u32 = 1 << 0;
        const CHUNK_END: u32 = 1 << 1;
        const PARENT: u32 = 1 << 2;
        const ROOT: u32 = 1 << 3;
        const KEYED_HASH: u32 = 1 << 4;
        const DERIVE_KEY_CONTEXT: u32 = 1 << 5;
        const DERIVE_KEY_MATERIAL: u32 = 1 << 6;

        const IV: [u32; 8] = [
            0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB,
            0x5BE0CD19,
        ];

        const MSG_PERMUTATION: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct Hash([u8; OUT_LEN]);

        impl Hash {
            pub fn as_bytes(&self) -> &[u8; OUT_LEN] {
                &self.0
            }

            pub fn to_bytes(self) -> [u8; OUT_LEN] {
                self.0
            }

            pub fn to_hex(&self) -> HexOutput {
                HexOutput(self.0)
            }
        }

        impl fmt::Debug for Hash {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple("Hash")
                    .field(&self.to_hex().to_string())
                    .finish()
            }
        }

        impl From<[u8; OUT_LEN]> for Hash {
            fn from(value: [u8; OUT_LEN]) -> Self {
                Self(value)
            }
        }

        impl From<Hash> for [u8; OUT_LEN] {
            fn from(value: Hash) -> Self {
                value.0
            }
        }

        impl AsRef<[u8]> for Hash {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }

        pub struct HexOutput([u8; OUT_LEN]);

        impl HexOutput {
            pub fn to_string(&self) -> String {
                encode_hex(&self.0)
            }
        }

        impl fmt::Display for HexOutput {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write_hex(f, &self.0)
            }
        }

        impl fmt::Debug for HexOutput {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write_hex(f, &self.0)
            }
        }

        fn encode_hex(bytes: &[u8]) -> String {
            let mut out = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                write!(&mut out, "{:02x}", byte).expect("write to string");
            }
            out
        }

        fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
            for byte in bytes {
                write!(f, "{:02x}", byte)?;
            }
            Ok(())
        }

        fn g(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, mx: u32, my: u32) {
            state[a] = state[a].wrapping_add(state[b]).wrapping_add(mx);
            state[d] = (state[d] ^ state[a]).rotate_right(16);
            state[c] = state[c].wrapping_add(state[d]);
            state[b] = (state[b] ^ state[c]).rotate_right(12);
            state[a] = state[a].wrapping_add(state[b]).wrapping_add(my);
            state[d] = (state[d] ^ state[a]).rotate_right(8);
            state[c] = state[c].wrapping_add(state[d]);
            state[b] = (state[b] ^ state[c]).rotate_right(7);
        }

        fn round(state: &mut [u32; 16], m: &[u32; 16]) {
            g(state, 0, 4, 8, 12, m[0], m[1]);
            g(state, 1, 5, 9, 13, m[2], m[3]);
            g(state, 2, 6, 10, 14, m[4], m[5]);
            g(state, 3, 7, 11, 15, m[6], m[7]);

            g(state, 0, 5, 10, 15, m[8], m[9]);
            g(state, 1, 6, 11, 12, m[10], m[11]);
            g(state, 2, 7, 8, 13, m[12], m[13]);
            g(state, 3, 4, 9, 14, m[14], m[15]);
        }

        fn permute(m: &mut [u32; 16]) {
            let mut permuted = [0; 16];
            for i in 0..16 {
                permuted[i] = m[MSG_PERMUTATION[i]];
            }
            *m = permuted;
        }

        fn compress(
            chaining_value: &[u32; 8],
            block_words: &[u32; 16],
            counter: u64,
            block_len: u32,
            flags: u32,
        ) -> [u32; 16] {
            let counter_low = counter as u32;
            let counter_high = (counter >> 32) as u32;
            let mut state = [
                chaining_value[0],
                chaining_value[1],
                chaining_value[2],
                chaining_value[3],
                chaining_value[4],
                chaining_value[5],
                chaining_value[6],
                chaining_value[7],
                IV[0],
                IV[1],
                IV[2],
                IV[3],
                counter_low,
                counter_high,
                block_len,
                flags,
            ];
            let mut block = *block_words;

            round(&mut state, &block);
            permute(&mut block);
            round(&mut state, &block);
            permute(&mut block);
            round(&mut state, &block);
            permute(&mut block);
            round(&mut state, &block);
            permute(&mut block);
            round(&mut state, &block);
            permute(&mut block);
            round(&mut state, &block);
            permute(&mut block);
            round(&mut state, &block);

            for i in 0..8 {
                state[i] ^= state[i + 8];
                state[i + 8] ^= chaining_value[i];
            }
            state
        }

        fn first_8_words(output: [u32; 16]) -> [u32; 8] {
            output[0..8].try_into().expect("slice length")
        }

        fn words_from_little_endian_bytes(bytes: &[u8], words: &mut [u32]) {
            debug_assert_eq!(bytes.len(), 4 * words.len());
            for (four_bytes, word) in bytes.chunks_exact(4).zip(words) {
                *word = u32::from_le_bytes(four_bytes.try_into().expect("word"));
            }
        }

        #[derive(Clone)]
        struct ChunkState {
            chaining_value: [u32; 8],
            chunk_counter: u64,
            block: [u8; BLOCK_LEN],
            block_len: u8,
            blocks_compressed: u8,
            flags: u32,
        }

        impl ChunkState {
            fn new(key_words: [u32; 8], chunk_counter: u64, flags: u32) -> Self {
                Self {
                    chaining_value: key_words,
                    chunk_counter,
                    block: [0; BLOCK_LEN],
                    block_len: 0,
                    blocks_compressed: 0,
                    flags,
                }
            }

            fn len(&self) -> usize {
                BLOCK_LEN * self.blocks_compressed as usize + self.block_len as usize
            }

            fn start_flag(&self) -> u32 {
                if self.blocks_compressed == 0 {
                    CHUNK_START
                } else {
                    0
                }
            }

            fn update(&mut self, mut input: &[u8]) {
                while !input.is_empty() {
                    if self.block_len as usize == BLOCK_LEN {
                        let mut block_words = [0; 16];
                        words_from_little_endian_bytes(&self.block, &mut block_words);
                        self.chaining_value = first_8_words(compress(
                            &self.chaining_value,
                            &block_words,
                            self.chunk_counter,
                            BLOCK_LEN as u32,
                            self.flags | self.start_flag(),
                        ));
                        self.blocks_compressed += 1;
                        self.block = [0; BLOCK_LEN];
                        self.block_len = 0;
                    }

                    let want = BLOCK_LEN - self.block_len as usize;
                    let take = min(want, input.len());
                    self.block[self.block_len as usize..][..take].copy_from_slice(&input[..take]);
                    self.block_len += take as u8;
                    input = &input[take..];
                }
            }

            fn output(&self) -> Output {
                let mut block_words = [0; 16];
                words_from_little_endian_bytes(&self.block, &mut block_words);
                Output {
                    input_chaining_value: self.chaining_value,
                    block_words,
                    counter: self.chunk_counter,
                    block_len: self.block_len as u32,
                    flags: self.flags | self.start_flag() | CHUNK_END,
                }
            }
        }

        fn parent_output(
            left_child_cv: [u32; 8],
            right_child_cv: [u32; 8],
            key_words: [u32; 8],
            flags: u32,
        ) -> Output {
            let mut block_words = [0; 16];
            block_words[..8].copy_from_slice(&left_child_cv);
            block_words[8..].copy_from_slice(&right_child_cv);
            Output {
                input_chaining_value: key_words,
                block_words,
                counter: 0,
                block_len: BLOCK_LEN as u32,
                flags: PARENT | flags,
            }
        }

        fn parent_cv(
            left_child_cv: [u32; 8],
            right_child_cv: [u32; 8],
            key_words: [u32; 8],
            flags: u32,
        ) -> [u32; 8] {
            parent_output(left_child_cv, right_child_cv, key_words, flags).chaining_value()
        }

        #[derive(Clone)]
        pub struct Hasher {
            chunk_state: ChunkState,
            key_words: [u32; 8],
            cv_stack: [[u32; 8]; 54],
            cv_stack_len: u8,
            flags: u32,
        }

        impl Hasher {
            fn new_internal(key_words: [u32; 8], flags: u32) -> Self {
                Self {
                    chunk_state: ChunkState::new(key_words, 0, flags),
                    key_words,
                    cv_stack: [[0; 8]; 54],
                    cv_stack_len: 0,
                    flags,
                }
            }

            pub fn new() -> Self {
                Self::new_internal(IV, 0)
            }

            pub fn new_keyed(key: &[u8; KEY_LEN]) -> Self {
                let mut key_words = [0; 8];
                words_from_little_endian_bytes(key, &mut key_words);
                Self::new_internal(key_words, KEYED_HASH)
            }

            pub fn new_derive_key(context: &str) -> Self {
                let mut context_hasher = Self::new_internal(IV, DERIVE_KEY_CONTEXT);
                context_hasher.update(context.as_bytes());
                let mut context_key = [0u8; KEY_LEN];
                context_hasher.finalize_xof(&mut context_key);
                let mut key_words = [0; 8];
                words_from_little_endian_bytes(&context_key, &mut key_words);
                Self::new_internal(key_words, DERIVE_KEY_MATERIAL)
            }

            fn push_stack(&mut self, cv: [u32; 8]) {
                self.cv_stack[self.cv_stack_len as usize] = cv;
                self.cv_stack_len += 1;
            }

            fn pop_stack(&mut self) -> [u32; 8] {
                self.cv_stack_len -= 1;
                self.cv_stack[self.cv_stack_len as usize]
            }

            fn add_chunk_chaining_value(&mut self, mut new_cv: [u32; 8], mut total_chunks: u64) {
                while total_chunks & 1 == 0 {
                    new_cv = parent_cv(self.pop_stack(), new_cv, self.key_words, self.flags);
                    total_chunks >>= 1;
                }
                self.push_stack(new_cv);
            }

            pub fn update(&mut self, mut input: &[u8]) {
                while !input.is_empty() {
                    if self.chunk_state.len() == CHUNK_LEN {
                        let chunk_cv = self.chunk_state.output().chaining_value();
                        let total_chunks = self.chunk_state.chunk_counter + 1;
                        self.add_chunk_chaining_value(chunk_cv, total_chunks);
                        self.chunk_state =
                            ChunkState::new(self.key_words, total_chunks, self.flags);
                    }

                    let want = CHUNK_LEN - self.chunk_state.len();
                    let take = min(want, input.len());
                    self.chunk_state.update(&input[..take]);
                    input = &input[take..];
                }
            }

            pub fn finalize(&self) -> Hash {
                let mut out = [0u8; OUT_LEN];
                self.finalize_xof(&mut out);
                Hash(out)
            }

            pub fn finalize_xof(&self, out_slice: &mut [u8]) {
                let mut output = self.chunk_state.output();
                let mut parents = self.cv_stack_len as usize;
                while parents > 0 {
                    parents -= 1;
                    output = parent_output(
                        self.cv_stack[parents],
                        output.chaining_value(),
                        self.key_words,
                        self.flags,
                    );
                }
                output.root_output_bytes(out_slice);
            }
        }

        impl Default for Hasher {
            fn default() -> Self {
                Self::new()
            }
        }

        struct Output {
            input_chaining_value: [u32; 8],
            block_words: [u32; 16],
            counter: u64,
            block_len: u32,
            flags: u32,
        }

        impl Output {
            fn chaining_value(&self) -> [u32; 8] {
                first_8_words(compress(
                    &self.input_chaining_value,
                    &self.block_words,
                    self.counter,
                    self.block_len,
                    self.flags,
                ))
            }

            fn root_output_bytes(&self, out_slice: &mut [u8]) {
                let mut output_block_counter = 0;
                for out_block in out_slice.chunks_mut(2 * OUT_LEN) {
                    let words = compress(
                        &self.input_chaining_value,
                        &self.block_words,
                        output_block_counter,
                        self.block_len,
                        self.flags | ROOT,
                    );
                    for (word, out_word) in words.iter().zip(out_block.chunks_mut(4)) {
                        out_word.copy_from_slice(&word.to_le_bytes()[..out_word.len()]);
                    }
                    output_block_counter += 1;
                }
            }
        }

        pub fn hash(input: &[u8]) -> Hash {
            let mut hasher = Hasher::new();
            hasher.update(input);
            hasher.finalize()
        }

        pub fn keyed_hash(key: &[u8; KEY_LEN], input: &[u8]) -> Hash {
            let mut hasher = Hasher::new_keyed(key);
            hasher.update(input);
            hasher.finalize()
        }

        pub fn derive_key(context: &str, material: &[u8]) -> [u8; KEY_LEN] {
            let mut hasher = Hasher::new_derive_key(context);
            hasher.update(material);
            let mut out = [0u8; KEY_LEN];
            hasher.finalize_xof(&mut out);
            out
        }

        pub fn xof(input: &[u8], out: &mut [u8]) {
            let mut hasher = Hasher::new();
            hasher.update(input);
            hasher.finalize_xof(out);
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            fn patterned_input(len: usize) -> Vec<u8> {
                let mut input = vec![0u8; len];
                for (idx, byte) in input.iter_mut().enumerate() {
                    *byte = (idx % 251) as u8;
                }
                input
            }

            #[test]
            fn hash_known_vectors() {
                let cases = [
                    (
                        0usize,
                        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
                    ),
                    (
                        1,
                        "2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213",
                    ),
                    (
                        2,
                        "7b7015bb92cf0b318037702a6cdd81dee41224f734684c2c122cd6359cb1ee63",
                    ),
                    (
                        3,
                        "e1be4d7a8ab5560aa4199eea339849ba8e293d55ca0a81006726d184519e647f",
                    ),
                    (
                        64,
                        "4eed7141ea4a5cd4b788606bd23f46e212af9cacebacdc7d1f4c6dc7f2511b98",
                    ),
                    (
                        65,
                        "de1e5fa0be70df6d2be8fffd0e99ceaa8eb6e8c93a63f2d8d1c30ecb6b263dee",
                    ),
                    (
                        1024,
                        "42214739f095a406f3fc83deb889744ac00df831c10daa55189b5d121c855af7",
                    ),
                ];
                for (len, expected_hex) in cases {
                    let digest = hash(&patterned_input(len));
                    assert_eq!(digest.to_hex().to_string(), expected_hex);
                }
            }

            #[test]
            fn keyed_and_derive_vectors() {
                let input = patterned_input(64);
                let key: [u8; KEY_LEN] = *b"whats the Elvish word for friend";
                let keyed = keyed_hash(&key, &input);
                assert_eq!(
                    keyed.to_hex().to_string(),
                    "ba8ced36f327700d213f120b1a207a3b8c04330528586f414d09f2f7d9ccb7e6"
                );

                let derived = derive_key("BLAKE3 2019-12-27 16:29:52 test vectors context", &input);
                assert_eq!(
                    encode_hex(&derived),
                    "a5c4a7053fa86b64746d4bb688d06ad1f02a18fce9afd3e818fefaa7126bf73e"
                );
            }
        }
    }

    mod sha256_impl {
        const OUTPUT_LEN: usize = 32;
        const BLOCK_SIZE: usize = 64;

        pub fn digest(input: &[u8]) -> [u8; OUTPUT_LEN] {
            digest_chunks(&[input])
        }

        fn digest_chunks(chunks: &[&[u8]]) -> [u8; OUTPUT_LEN] {
            let mut state = State::new();
            for chunk in chunks {
                state.update(chunk);
            }
            state.finalize()
        }

        struct State {
            h: [u32; 8],
            buffer: [u8; BLOCK_SIZE],
            buffer_len: usize,
            bit_len: u64,
        }

        impl State {
            fn new() -> Self {
                Self {
                    h: INITIAL_HASH,
                    buffer: [0u8; BLOCK_SIZE],
                    buffer_len: 0,
                    bit_len: 0,
                }
            }

            fn update(&mut self, mut data: &[u8]) {
                if data.is_empty() {
                    return;
                }

                self.bit_len = self.bit_len.wrapping_add((data.len() as u64) << 3);

                if self.buffer_len > 0 {
                    let space = BLOCK_SIZE - self.buffer_len;
                    if data.len() >= space {
                        self.buffer[self.buffer_len..self.buffer_len + space]
                            .copy_from_slice(&data[..space]);
                        let block = self.buffer;
                        self.process_block(&block);
                        self.buffer_len = 0;
                        data = &data[space..];
                    } else {
                        self.buffer[self.buffer_len..self.buffer_len + data.len()]
                            .copy_from_slice(data);
                        self.buffer_len += data.len();
                        return;
                    }
                }

                while data.len() >= BLOCK_SIZE {
                    let mut block = [0u8; BLOCK_SIZE];
                    block.copy_from_slice(&data[..BLOCK_SIZE]);
                    self.process_block(&block);
                    data = &data[BLOCK_SIZE..];
                }

                if !data.is_empty() {
                    self.buffer[..data.len()].copy_from_slice(data);
                    self.buffer_len = data.len();
                }
            }

            fn finalize(mut self) -> [u8; OUTPUT_LEN] {
                self.buffer[self.buffer_len] = 0x80;
                self.buffer_len += 1;

                if self.buffer_len > BLOCK_SIZE - 8 {
                    for byte in &mut self.buffer[self.buffer_len..] {
                        *byte = 0;
                    }
                    let block = self.buffer;
                    self.process_block(&block);
                    self.buffer_len = 0;
                }

                for byte in &mut self.buffer[self.buffer_len..BLOCK_SIZE - 8] {
                    *byte = 0;
                }

                let bit_len_bytes = self.bit_len.to_be_bytes();
                self.buffer[BLOCK_SIZE - 8..BLOCK_SIZE].copy_from_slice(&bit_len_bytes);
                let block = self.buffer;
                self.process_block(&block);

                let mut out = [0u8; OUTPUT_LEN];
                for (chunk, value) in out.chunks_mut(4).zip(self.h.iter()) {
                    chunk.copy_from_slice(&value.to_be_bytes());
                }
                out
            }

            fn process_block(&mut self, block: &[u8; BLOCK_SIZE]) {
                let mut w = [0u32; 64];
                for (i, chunk) in block.chunks_exact(4).enumerate().take(16) {
                    w[i] = u32::from_be_bytes(chunk.try_into().expect("chunk"));
                }

                for t in 16..64 {
                    let s0 = small_sigma0(w[t - 15]);
                    let s1 = small_sigma1(w[t - 2]);
                    w[t] = w[t - 16]
                        .wrapping_add(s0)
                        .wrapping_add(w[t - 7])
                        .wrapping_add(s1);
                }

                let mut a = self.h[0];
                let mut b = self.h[1];
                let mut c = self.h[2];
                let mut d = self.h[3];
                let mut e = self.h[4];
                let mut f = self.h[5];
                let mut g = self.h[6];
                let mut h = self.h[7];

                for t in 0..64 {
                    let t1 = h
                        .wrapping_add(big_sigma1(e))
                        .wrapping_add(ch(e, f, g))
                        .wrapping_add(K[t])
                        .wrapping_add(w[t]);
                    let t2 = big_sigma0(a).wrapping_add(maj(a, b, c));

                    h = g;
                    g = f;
                    f = e;
                    e = d.wrapping_add(t1);
                    d = c;
                    c = b;
                    b = a;
                    a = t1.wrapping_add(t2);
                }

                self.h[0] = self.h[0].wrapping_add(a);
                self.h[1] = self.h[1].wrapping_add(b);
                self.h[2] = self.h[2].wrapping_add(c);
                self.h[3] = self.h[3].wrapping_add(d);
                self.h[4] = self.h[4].wrapping_add(e);
                self.h[5] = self.h[5].wrapping_add(f);
                self.h[6] = self.h[6].wrapping_add(g);
                self.h[7] = self.h[7].wrapping_add(h);
            }
        }

        #[inline(always)]
        fn ch(x: u32, y: u32, z: u32) -> u32 {
            (x & y) ^ ((!x) & z)
        }

        #[inline(always)]
        fn maj(x: u32, y: u32, z: u32) -> u32 {
            (x & y) ^ (x & z) ^ (y & z)
        }

        #[inline(always)]
        fn big_sigma0(x: u32) -> u32 {
            x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
        }

        #[inline(always)]
        fn big_sigma1(x: u32) -> u32 {
            x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
        }

        #[inline(always)]
        fn small_sigma0(x: u32) -> u32 {
            x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
        }

        #[inline(always)]
        fn small_sigma1(x: u32) -> u32 {
            x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
        }

        const INITIAL_HASH: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];

        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];

        #[cfg(test)]
        mod tests {
            use super::digest;

            #[test]
            fn sha256_matches_known_vector() {
                let data = b"abc";
                let expected = [
                    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d,
                    0xae, 0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10,
                    0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
                ];
                assert_eq!(digest(data), expected);
            }
        }
    }
}

/// Base-N encoders used across the stack.
pub mod base {
    use base64_fp::{decode_standard, encode_standard};

    /// Encode bytes as Base64.
    pub fn encode_base64(input: &[u8]) -> String {
        encode_standard(input)
    }

    /// Decode Base64 text into raw bytes.
    pub fn decode_base64(input: &str) -> Result<Vec<u8>, &'static str> {
        decode_standard(input).map_err(|_| "invalid base64 input")
    }
}

/// Numeric helpers including an in-house FFT implementation.
pub mod math {
    use core::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};

    use std::f64::consts::PI;

    /// In-place radix-2 Cooley–Tukey FFT.
    pub fn fft(values: &mut [Complex]) {
        let n = values.len();
        if n <= 1 {
            return;
        }
        assert!(
            n.is_power_of_two(),
            "fft input length must be a power of two"
        );

        let bits = n.trailing_zeros() as usize;
        for i in 0..n {
            let j = bit_reverse(i, bits);
            if j > i {
                values.swap(i, j);
            }
        }

        let mut len = 2;
        while len <= n {
            let angle = -2.0 * PI / len as f64;
            let w_len = Complex::from_polar(1.0, angle);
            for start in (0..n).step_by(len) {
                let mut w = Complex::one();
                for offset in 0..(len / 2) {
                    let even = values[start + offset];
                    let odd = values[start + offset + len / 2];
                    let t = w * odd;
                    values[start + offset] = even + t;
                    values[start + offset + len / 2] = even - t;
                    w *= w_len;
                }
            }
            len <<= 1;
        }
    }

    fn bit_reverse(mut value: usize, bits: usize) -> usize {
        let mut reversed = 0;
        for _ in 0..bits {
            reversed = (reversed << 1) | (value & 1);
            value >>= 1;
        }
        reversed
    }

    /// Minimal complex number used by the FFT implementation.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Complex {
        pub re: f64,
        pub im: f64,
    }

    impl Complex {
        pub const fn new(re: f64, im: f64) -> Self {
            Self { re, im }
        }

        pub const fn one() -> Self {
            Self { re: 1.0, im: 0.0 }
        }

        fn from_polar(radius: f64, angle: f64) -> Self {
            Self {
                re: radius * angle.cos(),
                im: radius * angle.sin(),
            }
        }
    }

    impl Add for Complex {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                re: self.re + rhs.re,
                im: self.im + rhs.im,
            }
        }
    }

    impl AddAssign for Complex {
        fn add_assign(&mut self, rhs: Self) {
            self.re += rhs.re;
            self.im += rhs.im;
        }
    }

    impl Sub for Complex {
        type Output = Self;

        fn sub(self, rhs: Self) -> Self::Output {
            Self {
                re: self.re - rhs.re,
                im: self.im - rhs.im,
            }
        }
    }

    impl SubAssign for Complex {
        fn sub_assign(&mut self, rhs: Self) {
            self.re -= rhs.re;
            self.im -= rhs.im;
        }
    }

    impl Mul for Complex {
        type Output = Self;

        fn mul(self, rhs: Self) -> Self::Output {
            Self {
                re: self.re * rhs.re - self.im * rhs.im,
                im: self.re * rhs.im + self.im * rhs.re,
            }
        }
    }

    impl MulAssign for Complex {
        fn mul_assign(&mut self, rhs: Self) {
            *self = *self * rhs;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hash::{self, Blake3Hasher, BLAKE3_KEY_LEN};
    use super::math::{self, Complex};
    use super::rng::OsRng;
    use crypto_suite::hex;

    const BLAKE3_KEY: [u8; BLAKE3_KEY_LEN] = *b"whats the Elvish word for friend";
    const BLAKE3_CONTEXT: &str = "BLAKE3 2019-12-27 16:29:52 test vectors context";

    #[test]
    fn os_rng_produces_entropy() {
        let mut rng = OsRng::default();
        let mut buf = [0u8; 32];
        rng.fill_bytes(&mut buf).expect("os rng fill");
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn blake3_empty_vector_matches_reference() {
        let expected = decode_hex::<32>(&HASH_CASE_EMPTY[..64]);
        assert_eq!(hash::blake3(b""), expected);
    }

    #[test]
    fn blake3_keyed_matches_reference() {
        let expected = decode_hex::<32>(&KEYED_CASE_EMPTY[..64]);
        let digest = hash::blake3_keyed(&BLAKE3_KEY, b"");
        assert_eq!(digest.to_bytes(), expected);
    }

    #[test]
    fn blake3_derive_key_matches_reference() {
        let expected = decode_hex::<32>(&DERIVE_CASE_EMPTY[..64]);
        let derived = hash::blake3_derive_key(BLAKE3_CONTEXT, b"");
        assert_eq!(derived, expected);
    }

    #[test]
    fn blake3_xof_matches_reference() {
        let expected = decode_hex::<64>(&HASH_CASE_EMPTY[..128]);
        let mut out = [0u8; 64];
        hash::blake3_xof(b"", &mut out);
        assert_eq!(out, expected);
    }

    #[test]
    fn blake3_streaming_matches_single_shot() {
        let input = patterned_input(1024);
        let mut hasher = Blake3Hasher::new();
        for chunk in input.chunks(13) {
            hasher.update(chunk);
        }
        let streaming = hasher.finalize().to_bytes();
        assert_eq!(streaming, hash::blake3(&input));
    }

    #[test]
    fn sha256_vector_matches_reference() {
        const EXPECTED: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
            0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
            0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(hash::sha256(b"hello"), EXPECTED);
    }

    #[test]
    fn fft_impulse_response_is_flat() {
        let mut data = [
            Complex::new(1.0, 0.0),
            Complex::new(0.0, 0.0),
            Complex::new(0.0, 0.0),
            Complex::new(0.0, 0.0),
        ];
        math::fft(&mut data);
        for value in data.iter() {
            approx(value.re, 1.0);
            approx(value.im, 0.0);
        }
    }

    #[test]
    fn fft_constant_signal_collapses_to_dc() {
        let mut data = [Complex::new(1.0, 0.0); 4];
        math::fft(&mut data);
        approx(data[0].re, 4.0);
        approx(data[0].im, 0.0);
        for value in &data[1..] {
            approx(value.re, 0.0);
            approx(value.im, 0.0);
        }
    }

    fn decode_hex<const N: usize>(value: &str) -> [u8; N] {
        hex::decode_array::<N>(value).expect("valid fixture hex")
    }

    fn patterned_input(len: usize) -> Vec<u8> {
        let mut input = vec![0u8; len];
        for (idx, byte) in input.iter_mut().enumerate() {
            *byte = (idx % 251) as u8;
        }
        input
    }

    fn approx(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    const HASH_CASE_EMPTY: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262e00f03e7b69af26b7faaf09fcd333050338ddfe085b8cc869ca98b206c08243a26f5487789e8f660afe6c99ef9e0c52b92e7393024a80459cf91f476f9ffdbda7001c22e159b402631f277ca96f2defdf1078282314e763699a31c5363165421cce14d";
    const KEYED_CASE_EMPTY: &str = "92b2b75604ed3c761f9d6f62392c8a9227ad0ea3f09573e783f1498a4ed60d26b18171a2f22a4b94822c701f107153dba24918c4bae4d2945c20ece13387627d3b73cbf97b797d5e59948c7ef788f54372df45e45e4293c7dc18c1d41144a9758be58960856be1eabbe22c2653190de560ca3b2ac4aa692a9210694254c371e851bc8f";
    const DERIVE_CASE_EMPTY: &str = "2cc39783c223154fea8dfb7c1b1660f2ac2dcbd1c1de8277b0b0dd39b7e50d7d905630c8be290dfcf3e6842f13bddd573c098c3f17361f1f206b8cad9d088aa4a3f746752c6b0ce6a83b0da81d59649257cdf8eb3e9f7d4998e41021fac119deefb896224ac99f860011f73609e6e0e4540f93b273e56547dfd3aa1a035ba6689d89a0";
}

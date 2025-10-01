use crate::error::CompressionError;

const MAX_LITERAL: usize = 128;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 130;
const TOKEN_MATCH: u8 = 0x80;

#[derive(Clone, Debug)]
pub struct HybridCompressor {
    window_size: usize,
    search_span: usize,
}

impl HybridCompressor {
    pub fn new(level: i32) -> Self {
        let clamped = level.clamp(0, 9) as usize;
        let window_size = 4096 + clamped * 1024;
        let search_span = (64 + clamped * 32).min(window_size);
        Self {
            window_size,
            search_span,
        }
    }

    pub fn stream_encoder(&self) -> HybridEncoder {
        HybridEncoder::new(self.window_size, self.search_span)
    }

    pub fn stream_decoder(&self) -> HybridDecoder {
        HybridDecoder::new(self.window_size)
    }

    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut encoder = self.stream_encoder();
        let mut out = Vec::with_capacity(data.len());
        encoder.push(data, &mut out)?;
        encoder.finish(&mut out)?;
        Ok(out)
    }

    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut decoder = self.stream_decoder();
        let mut out = Vec::with_capacity(data.len() * 2 + 1);
        decoder.push(data, &mut out)?;
        decoder.finish()?;
        Ok(out)
    }
}

pub struct HybridEncoder {
    window: Vec<u8>,
    buffer: Vec<u8>,
    literals: Vec<u8>,
    window_size: usize,
    search_span: usize,
}

impl HybridEncoder {
    fn new(window_size: usize, search_span: usize) -> Self {
        Self {
            window: Vec::with_capacity(window_size),
            buffer: Vec::new(),
            literals: Vec::new(),
            window_size,
            search_span,
        }
    }

    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<u8>) -> Result<(), CompressionError> {
        self.buffer.extend_from_slice(chunk);
        let mut index = 0usize;
        let limit = self
            .buffer
            .len()
            .saturating_sub(MAX_MATCH)
            .min(self.buffer.len());
        while index < limit {
            if let Some((offset, len)) = self.find_match(index) {
                let slice = self.buffer[index..index + len].to_vec();
                self.flush_literals(out);
                self.emit_match(out, offset, len);
                self.extend_window(&slice);
                index += len;
            } else {
                let byte = self.buffer[index];
                self.literals.push(byte);
                if self.literals.len() == MAX_LITERAL {
                    self.flush_literals(out);
                }
                self.extend_window(&[byte]);
                index += 1;
            }
        }
        if index > 0 {
            self.buffer.drain(0..index);
        }
        Ok(())
    }

    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), CompressionError> {
        let mut index = 0usize;
        while index < self.buffer.len() {
            if let Some((offset, len)) = self.find_match(index) {
                let slice = self.buffer[index..index + len].to_vec();
                self.flush_literals(out);
                self.emit_match(out, offset, len);
                self.extend_window(&slice);
                index += len;
            } else {
                let byte = self.buffer[index];
                self.literals.push(byte);
                if self.literals.len() == MAX_LITERAL {
                    self.flush_literals(out);
                }
                self.extend_window(&[byte]);
                index += 1;
            }
        }
        self.flush_literals(out);
        self.buffer.clear();
        Ok(())
    }

    fn find_match(&self, start: usize) -> Option<(usize, usize)> {
        if start >= self.buffer.len() || self.window.len() < MIN_MATCH {
            return None;
        }
        let lookahead = &self.buffer[start..];
        if lookahead.len() < MIN_MATCH {
            return None;
        }
        let max_len = lookahead.len().min(MAX_MATCH);
        let window_len = self.window.len();
        let span = self.search_span.min(window_len);
        let mut best_len = 0usize;
        let mut best_offset = 0usize;
        for offset in 1..=span {
            let window_start = window_len - offset;
            if self.window[window_start] != lookahead[0] {
                continue;
            }
            let mut length = 1usize;
            while length < max_len && window_start + length < window_len {
                if self.window[window_start + length] != lookahead[length] {
                    break;
                }
                length += 1;
            }
            if length >= MIN_MATCH && length > best_len {
                best_len = length;
                best_offset = offset;
                if best_len == max_len {
                    break;
                }
            }
        }
        if best_len >= MIN_MATCH {
            Some((best_offset, best_len))
        } else {
            None
        }
    }

    fn emit_match(&self, out: &mut Vec<u8>, offset: usize, len: usize) {
        let len_byte = (len - MIN_MATCH) as u8 & 0x7F;
        out.push(TOKEN_MATCH | len_byte);
        out.extend_from_slice(&(offset as u16).to_le_bytes());
    }

    fn flush_literals(&mut self, out: &mut Vec<u8>) {
        if self.literals.is_empty() {
            return;
        }
        let mut cursor = 0usize;
        while cursor < self.literals.len() {
            let take = (self.literals.len() - cursor).min(MAX_LITERAL);
            out.push((take - 1) as u8);
            out.extend_from_slice(&self.literals[cursor..cursor + take]);
            cursor += take;
        }
        self.literals.clear();
    }

    fn extend_window(&mut self, data: &[u8]) {
        self.window.extend_from_slice(data);
        if self.window.len() > self.window_size {
            let excess = self.window.len() - self.window_size;
            self.window.drain(0..excess);
        }
    }
}

pub struct HybridDecoder {
    window: Vec<u8>,
    pending: Vec<u8>,
    window_size: usize,
}

impl HybridDecoder {
    fn new(window_size: usize) -> Self {
        Self {
            window: Vec::with_capacity(window_size),
            pending: Vec::new(),
            window_size,
        }
    }

    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<u8>) -> Result<(), CompressionError> {
        self.pending.extend_from_slice(chunk);
        let mut index = 0usize;
        while index < self.pending.len() {
            let token = self.pending[index];
            if token & TOKEN_MATCH == 0 {
                let literal_len = (token as usize) + 1;
                if self.pending.len() < index + 1 + literal_len {
                    break;
                }
                let start = index + 1;
                let end = start + literal_len;
                let segment = self.pending[start..end].to_vec();
                out.extend_from_slice(&segment);
                self.extend_window(&segment);
                index = end;
            } else {
                if self.pending.len() < index + 3 {
                    break;
                }
                let len = (token as usize & 0x7F) + MIN_MATCH;
                let offset =
                    u16::from_le_bytes([self.pending[index + 1], self.pending[index + 2]]) as usize;
                if offset == 0 {
                    return Err(CompressionError::Decompress("lz77 offset zero".into()));
                }
                if offset > out.len() {
                    return Err(CompressionError::Decompress(
                        "lz77 offset out of range".into(),
                    ));
                }
                for _ in 0..len {
                    let src_index = out.len() - offset;
                    let byte = out[src_index];
                    out.push(byte);
                    self.extend_window(&[byte]);
                }
                index += 3;
            }
        }
        if index > 0 {
            self.pending.drain(0..index);
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), CompressionError> {
        if !self.pending.is_empty() {
            return Err(CompressionError::Decompress(
                "hybrid payload truncated".into(),
            ));
        }
        Ok(())
    }

    fn extend_window(&mut self, data: &[u8]) {
        self.window.extend_from_slice(data);
        if self.window.len() > self.window_size {
            let excess = self.window.len() - self.window_size;
            self.window.drain(0..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_small() {
        let compressor = HybridCompressor::new(3);
        let data = b"aaaaabbbbccccccccccdddddddddddd";
        let encoded = compressor.compress(data).unwrap();
        let decoded = compressor.decompress(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn round_trip_random() {
        let compressor = HybridCompressor::new(6);
        let mut data = vec![0u8; 4096];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = ((i * 31) ^ (i >> 3)) as u8;
        }
        let encoded = compressor.compress(&data).unwrap();
        let decoded = compressor.decompress(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn streaming_round_trip() {
        let compressor = HybridCompressor::new(4);
        let mut encoder = compressor.stream_encoder();
        let mut encoded = Vec::new();
        for chunk in [b"hello ".as_ref(), b"world".as_ref(), b"!".as_ref()] {
            encoder.push(chunk, &mut encoded).unwrap();
        }
        encoder.finish(&mut encoded).unwrap();

        let mut decoder = compressor.stream_decoder();
        let mut decoded = Vec::new();
        for chunk in encoded.chunks(3) {
            decoder.push(chunk, &mut decoded).unwrap();
        }
        decoder.finish().unwrap();
        assert_eq!(decoded, b"hello world!");
    }

    #[test]
    fn detect_truncated_payload() {
        let compressor = HybridCompressor::new(2);
        let mut encoder = compressor.stream_encoder();
        let mut encoded = Vec::new();
        encoder.push(b"sample", &mut encoded).unwrap();
        encoder.finish(&mut encoded).unwrap();
        let mut decoder = compressor.stream_decoder();
        let mut output = Vec::new();
        decoder
            .push(&encoded[..encoded.len() - 1], &mut output)
            .expect("accepts partial chunk");
        assert!(matches!(
            decoder.finish(),
            Err(CompressionError::Decompress(_))
        ));
    }
}

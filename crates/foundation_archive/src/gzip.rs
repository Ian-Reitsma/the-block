use std::collections::VecDeque;
use std::io::{self, Read, Write};

use crate::gzip::crc32::Crc32;

/// Encode `data` into the Gzip format using an uncompressed DEFLATE stream.
///
/// The encoder produces standards-compliant Gzip payloads by emitting stored
/// DEFLATE blocks. While this does not shrink the data, it keeps the
/// implementation deterministic and portable while the first-party compression
/// stack continues to mature.
#[must_use]
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut encoder =
        Encoder::new(Vec::with_capacity(data.len() + 18)).expect("Vec writers should never fail");
    encoder
        .write_all(data)
        .expect("Vec writers should never fail");
    encoder.finish_vec()
}

/// Decode `data` from the Gzip format, supporting stored, fixed Huffman, and
/// dynamic Huffman DEFLATE blocks.
pub fn decode(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = Decoder::new(std::io::Cursor::new(data))?;
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// Streaming encoder that wraps an arbitrary writer.
pub struct Encoder<W: Write> {
    writer: W,
    crc: Crc32,
    written: u32,
    finished: bool,
}

impl<W: Write> Encoder<W> {
    /// Create a new encoder that writes to `writer`.
    pub fn new(mut writer: W) -> io::Result<Self> {
        writer.write_all(&[0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff])?;
        Ok(Self {
            writer,
            crc: Crc32::new(),
            written: 0,
            finished: false,
        })
    }

    fn write_block(&mut self, data: &[u8], final_block: bool) -> io::Result<()> {
        debug_assert!(data.len() <= 0xFFFF);
        let header = if final_block { 0x01 } else { 0x00 };
        self.writer.write_all(&[header])?;
        let len = data.len() as u16;
        self.writer.write_all(&len.to_le_bytes())?;
        self.writer.write_all(&(!len).to_le_bytes())?;
        self.writer.write_all(data)?;
        Ok(())
    }

    /// Finish encoding and return the underlying writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.finish_inner()?;
        Ok(self.writer)
    }

    fn finish_inner(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.write_block(&[], true)?;
        let crc = self.crc.clone().finalize();
        self.writer.write_all(&crc.to_le_bytes())?;
        self.writer.write_all(&self.written.to_le_bytes())?;
        self.writer.flush()?;
        self.finished = true;
        Ok(())
    }
}

impl Encoder<Vec<u8>> {
    /// Finish encoding and return the owned buffer.
    #[must_use]
    pub fn finish_vec(mut self) -> Vec<u8> {
        self.finish_inner().expect("Vec writers should never fail");
        self.writer
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.finished {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "attempted to write after encoder finished",
            ));
        }

        let mut written = 0;
        while written < buf.len() {
            let remaining = buf.len() - written;
            let chunk = remaining.min(0xFFFF);
            let data = &buf[written..written + chunk];
            self.write_block(data, false)?;
            self.crc.update(data);
            self.written = self.written.wrapping_add(chunk as u32);
            written += chunk;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// Streaming decoder capable of reading payloads produced by [`Encoder`].
pub struct Decoder<R: Read> {
    reader: BitReader<R>,
    crc: Crc32,
    produced: u32,
    buffer: Vec<u8>,
    offset: usize,
    final_block_seen: bool,
    finished: bool,
    history: VecDeque<u8>,
}

impl<R: Read> Decoder<R> {
    /// Create a new decoder from `reader`.
    pub fn new(mut reader: R) -> io::Result<Self> {
        read_header(&mut reader)?;
        Ok(Self {
            reader: BitReader::new(reader),
            crc: Crc32::new(),
            produced: 0,
            buffer: Vec::new(),
            offset: 0,
            final_block_seen: false,
            finished: false,
            history: VecDeque::with_capacity(WINDOW_SIZE),
        })
    }

    fn read_block(&mut self) -> io::Result<()> {
        if self.final_block_seen {
            return Ok(());
        }

        self.buffer.clear();
        self.offset = 0;

        let final_block = self.reader.read_bit()?;
        let block_type = self.reader.read_bits(2)?;

        match block_type {
            0 => self.read_stored_block()?,
            1 => {
                let literals = FixedTables::fixed_literal();
                let distances = FixedTables::fixed_distance();
                self.read_compressed_block(&literals, &distances)?;
            }
            2 => self.read_dynamic_block()?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "reserved deflate block type",
                ))
            }
        }

        self.final_block_seen = final_block;
        Ok(())
    }

    fn read_trailer(&mut self) -> io::Result<()> {
        if !self.final_block_seen || self.finished {
            return Ok(());
        }
        self.reader.align_to_byte();
        let mut trailer = [0u8; 8];
        self.reader.read_exact(&mut trailer)?;
        let expected_crc = u32::from_le_bytes(trailer[..4].try_into().unwrap());
        let expected_size = u32::from_le_bytes(trailer[4..].try_into().unwrap());
        let actual_crc = self.crc.clone().finalize();
        if expected_crc != actual_crc {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "crc mismatch in gzip trailer",
            ));
        }
        if expected_size != self.produced {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "size mismatch in gzip trailer",
            ));
        }
        self.finished = true;
        Ok(())
    }
}

impl<R: Read> Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        if self.finished {
            return Ok(0);
        }

        while self.offset >= self.buffer.len() {
            if self.final_block_seen {
                self.read_trailer()?;
                if self.finished {
                    return Ok(0);
                }
            }
            self.read_block()?;
            if self.buffer.is_empty() && self.final_block_seen {
                self.read_trailer()?;
                if self.finished {
                    return Ok(0);
                }
            }
        }

        let remaining = &self.buffer[self.offset..];
        let to_copy = remaining.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
        self.crc.update(&remaining[..to_copy]);
        self.produced = self.produced.wrapping_add(to_copy as u32);
        self.offset += to_copy;
        if self.offset >= self.buffer.len() && self.final_block_seen {
            self.read_trailer()?;
        }
        Ok(to_copy)
    }
}

impl<R: Read> Decoder<R> {
    fn read_stored_block(&mut self) -> io::Result<()> {
        self.reader.align_to_byte();
        let len = self.reader.read_u16()?;
        let nlen = self.reader.read_u16()?;
        if len ^ nlen != 0xFFFF {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stored block length check failed",
            ));
        }

        self.buffer.resize(len as usize, 0);
        if len > 0 {
            self.reader.read_exact(&mut self.buffer)?;
        }
        for idx in 0..self.buffer.len() {
            let byte = self.buffer[idx];
            self.push_history(byte);
        }
        Ok(())
    }

    fn read_compressed_block(
        &mut self,
        litlen: &HuffmanTables,
        dist: &HuffmanTables,
    ) -> io::Result<()> {
        loop {
            let symbol = litlen.decode(&mut self.reader)?;
            match symbol {
                0..=255 => {
                    let byte = symbol as u8;
                    self.buffer.push(byte);
                    self.push_history(byte);
                }
                256 => break,
                257..=285 => {
                    let length = decode_length(symbol, &mut self.reader)?;
                    let distance_symbol = dist.decode(&mut self.reader)?;
                    if distance_symbol > 29 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid distance symbol",
                        ));
                    }
                    let distance = decode_distance(distance_symbol, &mut self.reader)?;
                    self.copy_from_history(length, distance)?;
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid literal/length symbol",
                    ));
                }
            }
        }
        Ok(())
    }

    fn read_dynamic_block(&mut self) -> io::Result<()> {
        let hlit = self.reader.read_bits(5)? + 257;
        let hdist = self.reader.read_bits(5)? + 1;
        let hclen = self.reader.read_bits(4)? + 4;

        let mut code_length_lengths = [0u8; 19];
        for i in 0..hclen {
            let idx = CODE_LENGTH_ORDER[i as usize];
            code_length_lengths[idx] = self.reader.read_bits(3)? as u8;
        }
        let code_length_table = HuffmanTables::from_lengths(&code_length_lengths)?;

        let mut lengths = Vec::with_capacity((hlit + hdist) as usize);
        while lengths.len() < (hlit + hdist) as usize {
            let symbol = code_length_table.decode(&mut self.reader)?;
            match symbol {
                0..=15 => lengths.push(symbol as u8),
                16 => {
                    let repeat = self.reader.read_bits(2)? + 3;
                    let last = *lengths.last().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "repeat with no previous code")
                    })?;
                    lengths.extend(std::iter::repeat(last).take(repeat as usize));
                }
                17 => {
                    let repeat = self.reader.read_bits(3)? + 3;
                    lengths.extend(std::iter::repeat(0).take(repeat as usize));
                }
                18 => {
                    let repeat = self.reader.read_bits(7)? + 11;
                    lengths.extend(std::iter::repeat(0).take(repeat as usize));
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid code length symbol",
                    ));
                }
            }
        }

        let expected = (hlit + hdist) as usize;
        if lengths.len() < expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "insufficient code lengths",
            ));
        }
        if lengths.len() > expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "too many code lengths",
            ));
        }

        let litlen_lengths = &lengths[..hlit as usize];
        let dist_lengths = &lengths[hlit as usize..expected];

        let litlen = HuffmanTables::from_lengths(litlen_lengths)?;
        let dist = HuffmanTables::from_lengths(dist_lengths)?;
        self.read_compressed_block(&litlen, &dist)
    }

    fn copy_from_history(&mut self, length: usize, distance: usize) -> io::Result<()> {
        if distance == 0 || distance > self.history.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid back-reference distance",
            ));
        }
        for _ in 0..length {
            let idx = self.history.len() - distance;
            let byte = self.history[idx];
            self.buffer.push(byte);
            self.push_history(byte);
        }
        Ok(())
    }

    fn push_history(&mut self, byte: u8) {
        if self.history.len() == WINDOW_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(byte);
    }
}

const WINDOW_SIZE: usize = 32 * 1024;
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

struct BitReader<R: Read> {
    reader: R,
    buffer: u64,
    bits: u8,
}

impl<R: Read> BitReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: 0,
            bits: 0,
        }
    }

    fn read_bit(&mut self) -> io::Result<bool> {
        Ok(self.read_bits(1)? != 0)
    }

    fn read_bits(&mut self, mut count: u8) -> io::Result<u16> {
        if count == 0 {
            return Ok(0);
        }
        let mut value = 0u16;
        let mut shift = 0;
        while count > 0 {
            if self.bits == 0 {
                let mut buf = [0u8; 1];
                let read = self.reader.read(&mut buf)?;
                if read == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "unexpected eof while reading bits",
                    ));
                }
                self.buffer |= (buf[0] as u64) << self.bits;
                self.bits += 8;
            }
            let take = count.min(self.bits);
            let mask = (1u16 << take) - 1;
            value |= ((self.buffer & (mask as u64)) as u16) << shift;
            self.buffer >>= take;
            self.bits -= take;
            count -= take;
            shift += take as u16;
        }
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        let rem = self.bits % 8;
        if rem != 0 {
            self.buffer >>= rem;
            self.bits -= rem;
        }
    }

    fn read_u16(&mut self) -> io::Result<u16> {
        self.align_to_byte();
        let mut bytes = [0u8; 2];
        self.read_exact(&mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut written = 0;
        if self.bits > 0 {
            while self.bits >= 8 && written < buf.len() {
                buf[written] = (self.buffer & 0xFF) as u8;
                self.buffer >>= 8;
                self.bits -= 8;
                written += 1;
            }
        }
        if written == buf.len() {
            return Ok(());
        }
        self.reader.read_exact(&mut buf[written..])
    }
}

#[derive(Clone)]
struct HuffmanTables {
    tables: Vec<Vec<Option<u16>>>,
    max_bits: u8,
}

impl HuffmanTables {
    fn from_lengths(lengths: &[u8]) -> io::Result<Self> {
        let max_bits = lengths.iter().copied().max().unwrap_or(0);
        if max_bits == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing huffman codes",
            ));
        }
        let mut tables = vec![Vec::new(); max_bits as usize + 1];
        let mut bl_count = vec![0u16; max_bits as usize + 1];
        for &len in lengths {
            if len > max_bits {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid code length",
                ));
            }
            if len != 0 {
                bl_count[len as usize] += 1;
            }
        }

        let mut next_code = vec![0u16; max_bits as usize + 1];
        let mut code = 0u16;
        for bits in 1..=max_bits {
            code = (code + bl_count[(bits - 1) as usize]) << 1;
            next_code[bits as usize] = code;
            tables[bits as usize] = vec![None; 1usize << bits];
        }

        for (symbol, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let bits = len as usize;
            let code = next_code[bits];
            next_code[bits] += 1;
            let rev = reverse_bits(code, len);
            tables[bits][rev as usize] = Some(symbol as u16);
        }

        Ok(Self { tables, max_bits })
    }

    fn decode<R: Read>(&self, reader: &mut BitReader<R>) -> io::Result<u16> {
        let mut code = 0u16;
        for len in 1..=self.max_bits {
            let bit = reader.read_bit()? as u16;
            code |= bit << (len - 1);
            if let Some(Some(symbol)) = self
                .tables
                .get(len as usize)
                .and_then(|t| t.get(code as usize))
            {
                return Ok(*symbol);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unable to decode huffman symbol",
        ))
    }
}

struct FixedTables;

impl FixedTables {
    fn fixed_literal() -> HuffmanTables {
        let mut lengths = [0u8; 288];
        for i in 0..144 {
            lengths[i] = 8;
        }
        for i in 144..256 {
            lengths[i] = 9;
        }
        for i in 256..280 {
            lengths[i] = 7;
        }
        for i in 280..288 {
            lengths[i] = 8;
        }
        HuffmanTables::from_lengths(&lengths).expect("fixed literal table")
    }

    fn fixed_distance() -> HuffmanTables {
        let lengths = [5u8; 32];
        HuffmanTables::from_lengths(&lengths).expect("fixed distance table")
    }
}

fn reverse_bits(mut code: u16, len: u8) -> u16 {
    let mut result = 0u16;
    for _ in 0..len {
        result = (result << 1) | (code & 1);
        code >>= 1;
    }
    result
}

fn decode_length<R: Read>(symbol: u16, reader: &mut BitReader<R>) -> io::Result<usize> {
    const BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];

    let idx = (symbol - 257) as usize;
    let base = BASE[idx];
    let extra = EXTRA[idx];
    let extra_bits = if extra == 0 {
        0
    } else {
        reader.read_bits(extra)? as usize
    };
    Ok(base + extra_bits)
}

fn decode_distance<R: Read>(symbol: u16, reader: &mut BitReader<R>) -> io::Result<usize> {
    const BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    let base = BASE[symbol as usize];
    let extra = EXTRA[symbol as usize];
    let extra_bits = if extra == 0 {
        0
    } else {
        reader.read_bits(extra)? as usize
    };
    Ok(base + extra_bits)
}

fn read_header<R: Read>(reader: &mut R) -> io::Result<()> {
    let mut header = [0u8; 10];
    reader.read_exact(&mut header)?;
    if header[0] != 0x1f || header[1] != 0x8b {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid gzip magic",
        ));
    }
    if header[2] != 0x08 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported gzip compression method",
        ));
    }
    let flags = header[3];
    skip_optional_fields(reader, flags)?;
    Ok(())
}

fn skip_optional_fields<R: Read>(reader: &mut R, flags: u8) -> io::Result<()> {
    if flags & 0x04 != 0 {
        let mut len = [0u8; 2];
        reader.read_exact(&mut len)?;
        let extra_len = u16::from_le_bytes(len);
        io::copy(&mut reader.take(extra_len.into()), &mut io::sink())?;
    }
    if flags & 0x08 != 0 {
        skip_c_string(reader)?;
    }
    if flags & 0x10 != 0 {
        skip_c_string(reader)?;
    }
    if flags & 0x02 != 0 {
        let mut crc16 = [0u8; 2];
        reader.read_exact(&mut crc16)?;
    }
    Ok(())
}

fn skip_c_string<R: Read>(reader: &mut R) -> io::Result<()> {
    let mut byte = [0u8; 1];
    loop {
        reader.read_exact(&mut byte)?;
        if byte[0] == 0 {
            break;
        }
    }
    Ok(())
}

mod crc32 {
    const POLY: u32 = 0xEDB8_8320;

    #[derive(Clone)]
    pub struct Crc32 {
        value: u32,
    }

    impl Crc32 {
        pub const fn new() -> Self {
            Self { value: 0xFFFF_FFFF }
        }

        pub fn update(&mut self, bytes: &[u8]) {
            for &byte in bytes {
                let idx = ((self.value ^ byte as u32) & 0xFF) as usize;
                self.value = (self.value >> 8) ^ TABLE[idx];
            }
        }

        pub fn finalize(self) -> u32 {
            !self.value
        }
    }

    static TABLE: [u32; 256] = generate_table();

    const fn generate_table() -> [u32; 256] {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            table[i] = entry(i as u32);
            i += 1;
        }
        table
    }

    const fn entry(value: u32) -> u32 {
        let mut j = 0;
        let mut crc = value;
        while j < 8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        crc
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, Decoder, Encoder};
    use std::io::{Read, Write};

    #[test]
    fn round_trip_small_payload() {
        let input = b"hello, gzip";
        let encoded = encode(input);
        let decoded = decode(&encoded).expect("decode should succeed");
        assert_eq!(decoded, input);
    }

    #[test]
    fn streaming_round_trip_large_payload() {
        let input = vec![0xAA; 200_000];
        let mut encoder = Encoder::new(Vec::new()).unwrap();
        for chunk in input.chunks(10_000) {
            encoder.write_all(chunk).unwrap();
        }
        let encoded = encoder.finish_vec();

        let mut decoder = Decoder::new(std::io::Cursor::new(&encoded)).unwrap();
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn decode_python_generated_gzip() {
        // Generated using Python's gzip module with compresslevel=0 to emit stored blocks.
        let encoded: &[u8] = b"\x1f\x8b\x08\x00\x17\xf1\xe8h\x00\xff\x01\x0b\x00\xf4\xffhello world\x85\x11J\r\x0b\x00\x00\x00";
        let decoded = decode(encoded).expect("decode should succeed");
        assert_eq!(decoded, b"hello world");
    }

    #[test]
    fn decode_dynamic_huffman_gzip() {
        let encoded: &[u8] = b"\x1f\x8b\x08\x00{\xf8\xe8h\x02\xffK\xa9\xccK\xcc\xcdLVH\xaf\xca,P(.-(\xc8/*QH*\xcdK\xc9I\x05\x00t\xc9\x99\x08\x1b\x00\x00\x00";
        let mut decoder = Decoder::new(std::io::Cursor::new(encoded)).expect("decoder");
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).expect("streaming decode");
        assert_eq!(out, b"dynamic gzip support bundle");
    }
}

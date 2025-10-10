use std::io::{self, Read, Write};
use std::path::Path;

const BLOCK_SIZE: usize = 512;
const ZERO_BLOCK: [u8; BLOCK_SIZE] = [0u8; BLOCK_SIZE];

/// Minimal TAR header supporting regular file entries.
#[derive(Clone, Debug)]
pub struct Header {
    size: u64,
    mode: u32,
    mtime: u64,
}

impl Header {
    /// Construct a header compatible with GNU tar defaults.
    #[must_use]
    pub fn new_gnu() -> Self {
        Self {
            size: 0,
            mode: 0o644,
            mtime: 0,
        }
    }

    /// Update the file size recorded in the header.
    pub fn set_size(&mut self, size: u64) {
        self.size = size;
    }

    /// Update the permission bits recorded in the header.
    pub fn set_mode(&mut self, mode: u32) {
        self.mode = mode;
    }

    /// Update the modification timestamp recorded in the header.
    pub fn set_mtime(&mut self, mtime: u64) {
        self.mtime = mtime;
    }

    /// Traditional tar writers require callers to set the checksum after
    /// populating other fields.  Our implementation computes the checksum
    /// automatically, so this method is a no-op retained for API parity.
    pub fn set_cksum(&mut self) {}
}

/// Streaming TAR builder that writes directly to the provided sink.
pub struct Builder<W: Write> {
    writer: W,
    finished: bool,
}

impl<W: Write> Builder<W> {
    /// Create a new builder that writes entries into `writer`.
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            finished: false,
        }
    }

    /// Append a file entry to the archive.
    pub fn append_data<P, D>(&mut self, header: &mut Header, path: P, data: D) -> io::Result<()>
    where
        P: AsRef<Path>,
        D: AsRef<[u8]>,
    {
        let data = data.as_ref();
        let mut cursor = io::Cursor::new(data);
        self.append_reader(header, path, data.len() as u64, &mut cursor)
    }

    /// Append a file entry by streaming the payload from `reader`.
    pub fn append_reader<P, R>(
        &mut self,
        header: &mut Header,
        path: P,
        size: u64,
        reader: &mut R,
    ) -> io::Result<()>
    where
        P: AsRef<Path>,
        R: Read,
    {
        let name = path
            .as_ref()
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-utf8 path"))?;
        header.size = size;

        let mut block = [0u8; BLOCK_SIZE];
        write_name(&mut block, name)?;
        write_octal(&mut block[100..108], header.mode as u64);
        write_octal(&mut block[108..116], 0);
        write_octal(&mut block[116..124], 0);
        write_octal(&mut block[124..136], header.size);
        write_octal(&mut block[136..148], header.mtime);
        // Reserve checksum field with spaces before calculating the value.
        for byte in &mut block[148..156] {
            *byte = b' ';
        }
        block[156] = b'0'; // Regular file.
        block[257..263].copy_from_slice(b"ustar\0");
        block[263..265].copy_from_slice(b"00");

        let checksum: u32 = block.iter().map(|&b| b as u32).sum();
        let mut chk_field = [0u8; 8];
        write_octal(&mut chk_field, checksum as u64);
        block[148..156].copy_from_slice(&chk_field);

        self.writer.write_all(&block)?;
        let mut remaining = size;
        let mut buffer = [0u8; 8 * 1024];
        while remaining > 0 {
            let to_read = remaining.min(buffer.len() as u64) as usize;
            let read = reader.read(&mut buffer[..to_read])?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "reader ended before declaring size",
                ));
            }
            self.writer.write_all(&buffer[..read])?;
            remaining -= read as u64;
        }
        let block = BLOCK_SIZE as u64;
        let pad = (block - (size % block)) % block;
        if pad != 0 {
            self.writer.write_all(&ZERO_BLOCK[..pad as usize])?;
        }
        Ok(())
    }

    /// Finalise the archive by emitting the end-of-archive markers and
    /// returning the underlying writer.
    pub fn finish(mut self) -> io::Result<W> {
        if !self.finished {
            self.writer.write_all(&ZERO_BLOCK)?;
            self.writer.write_all(&ZERO_BLOCK)?;
            self.finished = true;
        }
        Ok(self.writer)
    }
}

/// Reader for TAR archives that yields fully materialised entries.
pub struct Reader<R: Read> {
    reader: R,
    finished: bool,
}

/// Archive entry consisting of a UTF-8 path and the full file contents.
pub struct Entry {
    name: String,
    data: Vec<u8>,
    size: u64,
}

impl Entry {
    /// Path of the entry within the archive.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Raw file contents.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Declared file size.
    pub fn size(&self) -> u64 {
        self.size
    }
}

impl<R: Read> Reader<R> {
    /// Construct a reader from the provided input stream.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            finished: false,
        }
    }

    /// Return the next entry in the archive, or `None` when exhausted.
    pub fn next(&mut self) -> io::Result<Option<Entry>> {
        if self.finished {
            return Ok(None);
        }

        let mut header = [0u8; BLOCK_SIZE];
        self.reader.read_exact(&mut header)?;
        if header.iter().all(|&b| b == 0) {
            self.finished = true;
            return Ok(None);
        }

        let name = parse_name(&header)?;
        let size = parse_size(&header)?;
        let typeflag = header[156];
        if !(typeflag == b'0' || typeflag == 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported tar entry type",
            ));
        }

        if size > (usize::MAX as u64) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry exceeds local address space",
            ));
        }

        let mut data = vec![0u8; size as usize];
        self.reader.read_exact(&mut data)?;
        let pad = (BLOCK_SIZE as u64 - (size % BLOCK_SIZE as u64)) % BLOCK_SIZE as u64;
        if pad > 0 {
            io::copy(&mut self.reader.by_ref().take(pad), &mut io::sink())?;
        }

        Ok(Some(Entry { name, data, size }))
    }
}

fn parse_name(block: &[u8; BLOCK_SIZE]) -> io::Result<String> {
    let name = read_c_string(&block[..100])?;
    let prefix = read_c_string(&block[345..500])?;
    if prefix.is_empty() {
        Ok(name)
    } else if name.is_empty() {
        Ok(prefix)
    } else {
        Ok(format!("{}/{}", prefix, name))
    }
}

fn read_c_string(buf: &[u8]) -> io::Result<String> {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    match std::str::from_utf8(&buf[..len]) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "non-utf8 path in tar header",
        )),
    }
}

fn parse_size(block: &[u8; BLOCK_SIZE]) -> io::Result<u64> {
    let field = &block[124..136];
    let mut value = 0u64;
    let mut seen_digit = false;
    for &byte in field {
        match byte {
            b'0'..=b'7' => {
                value = (value << 3) | u64::from(byte - b'0');
                seen_digit = true;
            }
            0 | b' ' => {
                if seen_digit {
                    break;
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid size field in tar header",
                ));
            }
        }
    }
    Ok(value)
}

fn write_name(block: &mut [u8; BLOCK_SIZE], name: &str) -> io::Result<()> {
    let bytes = name.as_bytes();
    if bytes.len() > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path exceeds tar header limit",
        ));
    }
    block[..bytes.len()].copy_from_slice(bytes);
    Ok(())
}

fn write_octal(buf: &mut [u8], mut value: u64) {
    if buf.is_empty() {
        return;
    }
    let width = buf.len() - 1; // Leave trailing space.
    for b in &mut buf[..width] {
        *b = b'0';
    }
    buf[buf.len() - 1] = b' ';
    let mut idx = width;
    loop {
        if idx == 0 {
            break;
        }
        idx -= 1;
        buf[idx] = b'0' + (value & 0x7) as u8;
        value >>= 3;
        if value == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Builder, Header, Reader, BLOCK_SIZE, ZERO_BLOCK};
    use std::io::{self, Read};

    #[derive(Clone)]
    struct ChunkReader {
        data: Vec<u8>,
        chunk: usize,
        pos: usize,
    }

    impl ChunkReader {
        fn new(data: Vec<u8>, chunk: usize) -> Self {
            Self {
                data,
                chunk,
                pos: 0,
            }
        }
    }

    impl Read for ChunkReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() {
                return Ok(0);
            }
            let remaining = self.data.len() - self.pos;
            let to_copy = remaining.min(self.chunk).min(buf.len());
            buf[..to_copy].copy_from_slice(&self.data[self.pos..self.pos + to_copy]);
            self.pos += to_copy;
            Ok(to_copy)
        }
    }

    #[test]
    fn append_reader_matches_append_data() {
        let payload: Vec<u8> = (0u8..=255).collect();

        let mut header_vec = Header::new_gnu();
        let mut builder_vec = Builder::new(Vec::new());
        builder_vec
            .append_data(&mut header_vec, "payload.bin", &payload)
            .unwrap();
        let archive_vec = builder_vec.finish().unwrap();

        let mut header_stream = Header::new_gnu();
        let mut builder_stream = Builder::new(Vec::new());
        let mut reader = ChunkReader::new(payload.clone(), 7);
        builder_stream
            .append_reader(
                &mut header_stream,
                "payload.bin",
                payload.len() as u64,
                &mut reader,
            )
            .unwrap();
        let archive_stream = builder_stream.finish().unwrap();

        assert_eq!(archive_vec, archive_stream);
    }

    #[test]
    fn append_reader_errors_on_short_input() {
        let data = vec![1u8; 10];
        let mut reader = ChunkReader::new(data.clone(), 3);
        // Shrink reader so it stops early.
        reader.data.truncate(5);

        let mut header = Header::new_gnu();
        let mut builder = Builder::new(Vec::new());
        let err = builder
            .append_reader(&mut header, "file.bin", data.len() as u64, &mut reader)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn builder_finish_writes_trailer() {
        let mut header = Header::new_gnu();
        let mut builder = Builder::new(Vec::new());
        builder.append_data(&mut header, "empty.txt", &[]).unwrap();
        let archive = builder.finish().unwrap();

        // Trailer consists of two zero blocks.
        let trailer = &archive[archive.len() - 2 * BLOCK_SIZE..];
        assert_eq!(trailer, &[ZERO_BLOCK, ZERO_BLOCK].concat());
    }

    #[test]
    fn reader_yields_entries() {
        let mut header = Header::new_gnu();
        let mut builder = Builder::new(Vec::new());
        builder
            .append_data(&mut header, "config.toml", b"admin=1")
            .unwrap();
        let archive = builder.finish().unwrap();

        let mut reader = Reader::new(std::io::Cursor::new(archive));
        let mut entries = Vec::new();
        while let Some(entry) = reader.next().unwrap() {
            entries.push((entry.name().to_string(), entry.data().to_vec()));
        }
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "config.toml");
        assert_eq!(entries[0].1, b"admin=1");
    }
}

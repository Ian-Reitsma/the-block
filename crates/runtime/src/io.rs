use crate::net::TcpStream;
use std::io::{self, Error, ErrorKind};

/// Buffered TCP stream helper that mirrors the subset of `tokio::io::BufReader`
/// functionality relied upon by the node while operating entirely on the
/// runtime-provided socket primitives.
pub struct BufferedTcpStream {
    stream: TcpStream,
    buffer: Vec<u8>,
    consumed: usize,
}

impl BufferedTcpStream {
    /// Wraps the provided runtime TCP stream with a small reusable buffer.
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buffer: Vec::with_capacity(1024),
            consumed: 0,
        }
    }

    /// Returns a shared reference to the underlying stream.
    pub fn get_ref(&self) -> &TcpStream {
        &self.stream
    }

    /// Returns a mutable reference to the underlying stream.
    pub fn get_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }

    /// Consumes the buffered reader and yields the inner stream.
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }

    /// Reads a single line into `line`, returning the number of bytes
    /// appended. Mirrors the behaviour of `BufRead::read_line` by yielding
    /// partial data when EOF is reached without a trailing newline.
    pub async fn read_line(&mut self, line: &mut String) -> io::Result<usize> {
        let initial_len = line.len();
        loop {
            if let Some(pos) = self.available().iter().position(|&b| b == b'\n') {
                let end = pos + 1;
                {
                    let available = self.available();
                    self.push_chunk(line, &available[..end])?;
                }
                self.consume(end);
                return Ok(line.len() - initial_len);
            }

            let mut temp = [0u8; 1024];
            let read = self.stream.read(&mut temp).await?;
            if read == 0 {
                if self.available().is_empty() {
                    return Ok(0);
                }
                let consumed = {
                    let available = self.available();
                    self.push_chunk(line, available)?;
                    available.len()
                };
                self.consume(consumed);
                return Ok(line.len() - initial_len);
            }
            self.buffer.extend_from_slice(&temp[..read]);
        }
    }

    /// Reads exactly `buf.len()` bytes into `buf`, leveraging any buffered
    /// data that has already been read ahead of the caller.
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut offset = 0;
        if !self.available().is_empty() {
            let to_copy = {
                let available = self.available();
                let to_copy = buf.len().min(available.len());
                buf[..to_copy].copy_from_slice(&available[..to_copy]);
                to_copy
            };
            self.consume(to_copy);
            offset += to_copy;
        }
        while offset < buf.len() {
            let read = self.stream.read(&mut buf[offset..]).await?;
            if read == 0 {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "tcp stream closed before filling buffer",
                ));
            }
            offset += read;
        }
        Ok(())
    }

    /// Reads a big-endian length-prefixed frame from the stream. Returns
    /// `Ok(None)` when the peer cleanly closes the connection before another
    /// frame is available.
    pub async fn read_length_prefixed(&mut self, max_len: usize) -> io::Result<Option<Vec<u8>>> {
        self.ensure_buffered(4).await?;
        let available = self.available();
        if available.is_empty() {
            return Ok(None);
        }
        if available.len() < 4 {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "frame truncated while reading length prefix",
            ));
        }
        let mut prefix = [0u8; 4];
        prefix.copy_from_slice(&available[..4]);
        let len = u32::from_be_bytes(prefix) as usize;
        self.consume(4);
        if len > max_len {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("frame length {len} exceeds limit {max_len}"),
            ));
        }
        if len == 0 {
            return Ok(Some(Vec::new()));
        }
        self.ensure_buffered(len).await?;
        if self.available().len() < len {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "frame truncated while reading payload",
            ));
        }
        let mut data = vec![0u8; len];
        {
            let available = self.available();
            data.copy_from_slice(&available[..len]);
        }
        self.consume(len);
        Ok(Some(data))
    }

    /// Writes the provided payload preceded by its big-endian length prefix.
    pub async fn write_length_prefixed(&mut self, payload: &[u8]) -> io::Result<()> {
        if payload.len() > u32::MAX as usize {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "frame too large to encode as u32",
            ));
        }
        let len = (payload.len() as u32).to_be_bytes();
        self.stream.write_all(&len).await?;
        self.stream.write_all(payload).await
    }

    /// Writes the entirety of `buf` to the underlying stream.
    pub async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.stream.write_all(buf).await
    }

    /// Flushes the underlying stream ensuring pending writes hit the socket.
    pub async fn flush(&mut self) -> io::Result<()> {
        self.stream.flush().await
    }

    /// Initiates an orderly shutdown of the connection.
    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.stream.shutdown().await
    }

    fn push_chunk(&self, line: &mut String, chunk: &[u8]) -> io::Result<()> {
        match std::str::from_utf8(chunk) {
            Ok(part) => {
                line.push_str(part);
                Ok(())
            }
            Err(err) => Err(Error::new(ErrorKind::InvalidData, err)),
        }
    }

    fn available(&self) -> &[u8] {
        &self.buffer[self.consumed..]
    }

    fn consume(&mut self, amount: usize) {
        debug_assert!(amount <= self.available().len());
        self.consumed += amount;
        self.recycle_buffer();
    }

    async fn ensure_buffered(&mut self, needed: usize) -> io::Result<()> {
        let mut temp = [0u8; 1024];
        while self.available().len() < needed {
            let read = self.stream.read(&mut temp).await?;
            if read == 0 {
                break;
            }
            self.buffer.extend_from_slice(&temp[..read]);
        }
        Ok(())
    }

    fn recycle_buffer(&mut self) {
        if self.consumed == 0 {
            return;
        }
        if self.consumed >= self.buffer.len() {
            self.buffer.clear();
            self.consumed = 0;
            return;
        }
        if self.consumed > self.buffer.len() / 2 || self.buffer.len() > 4096 {
            let remaining = self.buffer.len() - self.consumed;
            self.buffer.copy_within(self.consumed.., 0);
            self.buffer.truncate(remaining);
            self.consumed = 0;
        }
    }
}

impl From<TcpStream> for BufferedTcpStream {
    fn from(stream: TcpStream) -> Self {
        Self::new(stream)
    }
}

/// Reads the entire stream into `buf`, returning the total number of bytes
/// appended. This mirrors `tokio::io::AsyncReadExt::read_to_end` without
/// relying on Tokio traits.
pub async fn read_to_end(stream: &mut TcpStream, buf: &mut Vec<u8>) -> io::Result<usize> {
    let mut total = 0usize;
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
        total += read;
    }
    Ok(total)
}

/// Reads a single length-prefixed frame without additional buffering.
/// Returns `Ok(None)` when the peer closes the connection before another
/// frame arrives.
pub async fn read_length_prefixed(
    stream: &mut TcpStream,
    max_len: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0u8; 4];
    let mut read = 0usize;
    while read < prefix.len() {
        let n = stream.read(&mut prefix[read..]).await?;
        if n == 0 {
            if read == 0 {
                return Ok(None);
            }
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "frame truncated while reading length prefix",
            ));
        }
        read += n;
    }
    let len = u32::from_be_bytes(prefix) as usize;
    if len > max_len {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("frame length {len} exceeds limit {max_len}"),
        ));
    }
    if len == 0 {
        return Ok(Some(Vec::new()));
    }
    let mut data = vec![0u8; len];
    let mut filled = 0usize;
    while filled < data.len() {
        let n = stream.read(&mut data[filled..]).await?;
        if n == 0 {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "frame truncated while reading payload",
            ));
        }
        filled += n;
    }
    Ok(Some(data))
}

/// Writes the supplied payload with a big-endian u32 length prefix.
pub async fn write_length_prefixed(stream: &mut TcpStream, payload: &[u8]) -> io::Result<()> {
    if payload.len() > u32::MAX as usize {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "frame too large to encode as u32",
        ));
    }
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(payload).await
}

use std::io::{self, ErrorKind};
use std::net::SocketAddr;
#[cfg(feature = "tokio-backend")]
use std::pin::Pin;
#[cfg(feature = "tokio-backend")]
use std::task::{Context, Poll};

#[cfg(feature = "tokio-backend")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(feature = "tokio-backend")]
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[cfg(feature = "inhouse-backend")]
use crate::inhouse;

pub struct TcpListener {
    inner: TcpListenerInner,
}

enum TcpListenerInner {
    #[cfg(feature = "inhouse-backend")]
    InHouse(inhouse::net::TcpListener),
    #[cfg(feature = "tokio-backend")]
    Tokio(tokio::net::TcpListener),
    #[cfg(feature = "stub-backend")]
    Stub(StubTcpListener),
}

pub struct TcpStream {
    inner: TcpStreamInner,
}

enum TcpStreamInner {
    #[cfg(feature = "inhouse-backend")]
    InHouse(inhouse::net::TcpStream),
    #[cfg(feature = "tokio-backend")]
    Tokio(tokio::net::TcpStream),
    #[cfg(feature = "stub-backend")]
    Stub(StubTcpStream),
}

pub struct UdpSocket {
    inner: UdpSocketInner,
}

enum UdpSocketInner {
    #[cfg(feature = "inhouse-backend")]
    InHouse(inhouse::net::UdpSocket),
    #[cfg(feature = "tokio-backend")]
    Tokio(tokio::net::UdpSocket),
    #[cfg(feature = "stub-backend")]
    Stub(StubUdpSocket),
}

impl TcpListener {
    pub async fn bind(addr: SocketAddr) -> io::Result<Self> {
        let handle = crate::handle();
        #[cfg(feature = "inhouse-backend")]
        if let Some(rt) = handle.inhouse_runtime() {
            let listener = inhouse::net::TcpListener::bind(rt.as_ref(), addr)?;
            return Ok(Self {
                inner: TcpListenerInner::InHouse(listener),
            });
        }
        #[cfg(feature = "tokio-backend")]
        if let Some(_rt) = handle.tokio_runtime() {
            let listener = tokio::net::TcpListener::bind(addr).await?;
            return Ok(Self {
                inner: TcpListenerInner::Tokio(listener),
            });
        }
        #[cfg(feature = "stub-backend")]
        {
            let listener = StubTcpListener::bind(addr)?;
            return Ok(Self {
                inner: TcpListenerInner::Stub(listener),
            });
        }
        #[allow(unreachable_code)]
        Err(io::Error::new(
            ErrorKind::Other,
            "no tcp listener backend available",
        ))
    }

    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpListenerInner::InHouse(listener) => {
                let (stream, addr) = listener.accept().await?;
                Ok((
                    TcpStream {
                        inner: TcpStreamInner::InHouse(stream),
                    },
                    addr,
                ))
            }
            #[cfg(feature = "tokio-backend")]
            TcpListenerInner::Tokio(listener) => {
                let (stream, addr) = listener.accept().await?;
                Ok((
                    TcpStream {
                        inner: TcpStreamInner::Tokio(stream),
                    },
                    addr,
                ))
            }
            #[cfg(feature = "stub-backend")]
            TcpListenerInner::Stub(listener) => {
                let (stream, addr) = listener.accept().await?;
                Ok((
                    TcpStream {
                        inner: TcpStreamInner::Stub(stream),
                    },
                    addr,
                ))
            }
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpListenerInner::InHouse(listener) => listener.local_addr(),
            #[cfg(feature = "tokio-backend")]
            TcpListenerInner::Tokio(listener) => listener.local_addr(),
            #[cfg(feature = "stub-backend")]
            TcpListenerInner::Stub(listener) => listener.local_addr(),
        }
    }
}

impl TcpStream {
    pub async fn connect(addr: SocketAddr) -> io::Result<Self> {
        let handle = crate::handle();
        #[cfg(feature = "inhouse-backend")]
        if let Some(rt) = handle.inhouse_runtime() {
            let future = inhouse::net::TcpStream::connect(rt.as_ref(), addr)?;
            let stream = future.await?;
            return Ok(Self {
                inner: TcpStreamInner::InHouse(stream),
            });
        }
        #[cfg(feature = "tokio-backend")]
        if let Some(_rt) = handle.tokio_runtime() {
            let stream = tokio::net::TcpStream::connect(addr).await?;
            return Ok(Self {
                inner: TcpStreamInner::Tokio(stream),
            });
        }
        #[cfg(feature = "stub-backend")]
        {
            let stream = StubTcpStream::connect(addr).await?;
            return Ok(Self {
                inner: TcpStreamInner::Stub(stream),
            });
        }
        #[allow(unreachable_code)]
        Err(io::Error::new(
            ErrorKind::Other,
            "no tcp stream backend available",
        ))
    }

    #[cfg(feature = "tokio-backend")]
    pub fn from_tokio(stream: tokio::net::TcpStream) -> Self {
        Self {
            inner: TcpStreamInner::Tokio(stream),
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.read(buf).await,
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => stream.read(buf).await,
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(stream) => stream.read(buf).await,
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.write(buf).await,
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => stream.write(buf).await,
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(stream) => stream.write(buf).await,
        }
    }

    #[cfg(feature = "tokio-backend")]
    fn poll_write_internal(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.poll_write(cx, buf),
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(_) => Poll::Ready(Err(io::Error::new(
                ErrorKind::Unsupported,
                "tcp stream unsupported on stub runtime",
            ))),
        }
    }

    #[cfg(feature = "tokio-backend")]
    fn poll_flush_internal(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.poll_flush(cx),
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(_) => Poll::Ready(Err(io::Error::new(
                ErrorKind::Unsupported,
                "tcp stream unsupported on stub runtime",
            ))),
        }
    }

    #[cfg(feature = "tokio-backend")]
    fn poll_shutdown_internal(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.poll_shutdown(cx),
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(_) => Poll::Ready(Err(io::Error::new(
                ErrorKind::Unsupported,
                "tcp stream unsupported on stub runtime",
            ))),
        }
    }

    pub async fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut offset = 0;
        while offset < buf.len() {
            let read = self.read(&mut buf[offset..]).await?;
            if read == 0 {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "tcp stream reached eof",
                ));
            }
            offset += read;
        }
        Ok(())
    }

    pub async fn write_all(&mut self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            let written = self.write(buf).await?;
            if written == 0 {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "tcp stream failed to write remaining bytes",
                ));
            }
            buf = &buf[written..];
        }
        Ok(())
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.flush().await,
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => stream.flush().await,
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(stream) => stream.flush().await,
        }
    }

    pub async fn shutdown(&mut self) -> io::Result<()> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.shutdown().await,
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => {
                stream.shutdown().await?;
                Ok(())
            }
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(stream) => stream.shutdown().await,
        }
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.peer_addr(),
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => stream.peer_addr(),
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(stream) => stream.peer_addr(),
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => stream.local_addr(),
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => stream.local_addr(),
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(stream) => stream.local_addr(),
        }
    }
}

impl UdpSocket {
    pub async fn bind(addr: SocketAddr) -> io::Result<Self> {
        let handle = crate::handle();
        #[cfg(feature = "inhouse-backend")]
        if let Some(rt) = handle.inhouse_runtime() {
            let socket = inhouse::net::UdpSocket::bind(rt.as_ref(), addr)?;
            return Ok(Self {
                inner: UdpSocketInner::InHouse(socket),
            });
        }
        #[cfg(feature = "tokio-backend")]
        if let Some(_rt) = handle.tokio_runtime() {
            let socket = tokio::net::UdpSocket::bind(addr).await?;
            return Ok(Self {
                inner: UdpSocketInner::Tokio(socket),
            });
        }
        #[cfg(feature = "stub-backend")]
        {
            let socket = StubUdpSocket::bind(addr)?;
            return Ok(Self {
                inner: UdpSocketInner::Stub(socket),
            });
        }
        #[allow(unreachable_code)]
        Err(io::Error::new(
            ErrorKind::Other,
            "no udp socket backend available",
        ))
    }

    pub async fn recv_from(&mut self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            UdpSocketInner::InHouse(socket) => socket.recv_from(buf).await,
            #[cfg(feature = "tokio-backend")]
            UdpSocketInner::Tokio(socket) => socket.recv_from(buf).await,
            #[cfg(feature = "stub-backend")]
            UdpSocketInner::Stub(socket) => socket.recv_from(buf).await,
        }
    }

    pub async fn send_to(&mut self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            UdpSocketInner::InHouse(socket) => socket.send_to(buf, addr).await,
            #[cfg(feature = "tokio-backend")]
            UdpSocketInner::Tokio(socket) => socket.send_to(buf, addr).await,
            #[cfg(feature = "stub-backend")]
            UdpSocketInner::Stub(socket) => socket.send_to(buf, addr).await,
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            UdpSocketInner::InHouse(socket) => socket.local_addr(),
            #[cfg(feature = "tokio-backend")]
            UdpSocketInner::Tokio(socket) => socket.local_addr(),
            #[cfg(feature = "stub-backend")]
            UdpSocketInner::Stub(socket) => socket.local_addr(),
        }
    }
}

#[cfg(feature = "tokio-backend")]
impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            TcpStreamInner::InHouse(stream) => {
                let slice = buf.initialize_unfilled();
                match stream.poll_read(cx, slice) {
                    Poll::Ready(Ok(read)) => {
                        buf.advance(read);
                        Poll::Ready(Ok(()))
                    }
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Pending => Poll::Pending,
                }
            }
            #[cfg(feature = "tokio-backend")]
            TcpStreamInner::Tokio(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(feature = "stub-backend")]
            TcpStreamInner::Stub(_) => Poll::Ready(Err(io::Error::new(
                ErrorKind::Unsupported,
                "tcp stream unsupported on stub runtime",
            ))),
        }
    }
}

#[cfg(feature = "tokio-backend")]
impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.poll_write_internal(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush_internal(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_shutdown_internal(cx)
    }
}

#[cfg(feature = "stub-backend")]
struct StubTcpListener;

#[cfg(feature = "stub-backend")]
impl StubTcpListener {
    fn bind(_addr: SocketAddr) -> io::Result<Self> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp listener unsupported on stub runtime",
        ))
    }

    async fn accept(&self) -> io::Result<(StubTcpStream, SocketAddr)> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp listener unsupported on stub runtime",
        ))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp listener unsupported on stub runtime",
        ))
    }
}

#[cfg(feature = "stub-backend")]
struct StubTcpStream;

#[cfg(feature = "stub-backend")]
impl StubTcpStream {
    async fn connect(_addr: SocketAddr) -> io::Result<Self> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream unsupported on stub runtime",
        ))
    }

    async fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream unsupported on stub runtime",
        ))
    }

    async fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream unsupported on stub runtime",
        ))
    }

    async fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream unsupported on stub runtime",
        ))
    }

    async fn shutdown(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream unsupported on stub runtime",
        ))
    }

    fn peer_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream unsupported on stub runtime",
        ))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream unsupported on stub runtime",
        ))
    }
}

#[cfg(feature = "stub-backend")]
struct StubUdpSocket;

#[cfg(feature = "stub-backend")]
impl StubUdpSocket {
    fn bind(_addr: SocketAddr) -> io::Result<Self> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket unsupported on stub runtime",
        ))
    }

    async fn recv_from(&mut self, _buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket unsupported on stub runtime",
        ))
    }

    async fn send_to(&mut self, _buf: &[u8], _addr: SocketAddr) -> io::Result<usize> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket unsupported on stub runtime",
        ))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket unsupported on stub runtime",
        ))
    }
}

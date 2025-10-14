use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::time::Duration;

pub struct TcpStream;

impl TcpStream {
    pub fn connect(_addr: SocketAddr) -> io::Result<(Self, bool)> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        Ok(None)
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    pub fn shutdown(&self, _how: Shutdown) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    pub fn set_nodelay(&self, _nodelay: bool) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    pub fn set_ttl(&self, _ttl: u32) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    pub fn set_read_timeout(&self, _dur: Option<Duration>) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    pub fn set_write_timeout(&self, _dur: Option<Duration>) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }
}

impl Read for TcpStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }
}

impl Write for TcpStream {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp stream is not supported on this platform",
        ))
    }
}

pub struct TcpListener;

impl TcpListener {
    pub fn bind(_addr: SocketAddr) -> io::Result<Self> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp listener is not supported on this platform",
        ))
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp listener is not supported on this platform",
        ))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "tcp listener is not supported on this platform",
        ))
    }
}

pub struct UdpSocket;

impl UdpSocket {
    pub fn bind(_addr: SocketAddr) -> io::Result<Self> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }

    pub fn recv_from(&self, _buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }

    pub fn send_to(&self, _buf: &[u8], _addr: SocketAddr) -> io::Result<usize> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }

    pub fn set_broadcast(&self, _broadcast: bool) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }

    pub fn set_multicast_ttl_v4(&self, _ttl: u32) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }

    pub fn set_read_timeout(&self, _dur: Option<Duration>) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }

    pub fn set_write_timeout(&self, _dur: Option<Duration>) -> io::Result<()> {
        Err(io::Error::new(
            ErrorKind::Unsupported,
            "udp socket is not supported on this platform",
        ))
    }
}

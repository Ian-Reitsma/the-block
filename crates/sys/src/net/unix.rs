use std::io::{self, ErrorKind, Read, Write};
use std::mem::size_of;
use std::net::{Shutdown, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::time::Duration;

const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;
const SOCK_STREAM: i32 = 1;
const SOCK_DGRAM: i32 = 2;
const SOCK_CLOEXEC: i32 = 0o2000000;
const SOCK_NONBLOCK: i32 = 0o0004000;
const IPPROTO_TCP: i32 = 6;
const IPPROTO_UDP: i32 = 17;
const SOL_SOCKET: i32 = 1;
const SO_REUSEADDR: i32 = 2;

#[cfg(any(target_os = "linux", target_os = "android"))]
const EINPROGRESS: i32 = 115;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]
const EINPROGRESS: i32 = 36;

#[repr(C)]
struct SockAddr {
    sa_family: u16,
    sa_data: [u8; 14],
}

#[repr(C)]
struct sockaddr_in {
    sin_family: u16,
    sin_port: u16,
    sin_addr: in_addr,
    sin_zero: [u8; 8],
}

#[repr(C)]
struct in_addr {
    s_addr: u32,
}

#[repr(C)]
struct sockaddr_in6 {
    sin6_family: u16,
    sin6_port: u16,
    sin6_flowinfo: u32,
    sin6_addr: in6_addr,
    sin6_scope_id: u32,
}

#[repr(C)]
struct in6_addr {
    s6_addr: [u8; 16],
}

enum SockAddrInternal {
    V4(sockaddr_in),
    V6(sockaddr_in6),
}

impl SockAddrInternal {
    fn new(addr: SocketAddr) -> (Self, u32) {
        match addr {
            SocketAddr::V4(v4) => {
                let ip = v4.ip().octets();
                let sin = sockaddr_in {
                    sin_family: AF_INET as u16,
                    sin_port: v4.port().to_be(),
                    sin_addr: in_addr {
                        s_addr: u32::from_ne_bytes(ip),
                    },
                    sin_zero: [0; 8],
                };
                (Self::V4(sin), size_of::<sockaddr_in>() as u32)
            }
            SocketAddr::V6(v6) => {
                let sin6 = sockaddr_in6 {
                    sin6_family: AF_INET6 as u16,
                    sin6_port: v6.port().to_be(),
                    sin6_flowinfo: v6.flowinfo(),
                    sin6_addr: in6_addr {
                        s6_addr: v6.ip().octets(),
                    },
                    sin6_scope_id: v6.scope_id(),
                };
                (Self::V6(sin6), size_of::<sockaddr_in6>() as u32)
            }
        }
    }

    fn as_ptr(&self) -> *const SockAddr {
        match self {
            Self::V4(inner) => inner as *const _ as *const SockAddr,
            Self::V6(inner) => inner as *const _ as *const SockAddr,
        }
    }
}

extern "C" {
    fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
    fn connect(fd: i32, addr: *const SockAddr, len: u32) -> i32;
    fn bind(fd: i32, addr: *const SockAddr, len: u32) -> i32;
    fn listen(fd: i32, backlog: i32) -> i32;
    fn setsockopt(fd: i32, level: i32, optname: i32, optval: *const i32, optlen: u32) -> i32;
    fn close(fd: i32) -> i32;
}

fn create_socket(domain: i32, ty: i32, protocol: i32) -> io::Result<RawFd> {
    let fd = unsafe { socket(domain, ty | SOCK_CLOEXEC | SOCK_NONBLOCK, protocol) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

fn set_reuseaddr(fd: RawFd) -> io::Result<()> {
    let value: i32 = 1;
    let res = unsafe {
        setsockopt(
            fd,
            SOL_SOCKET,
            SO_REUSEADDR,
            &value,
            size_of::<i32>() as u32,
        )
    };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn bind_socket(fd: RawFd, addr: SocketAddr) -> io::Result<()> {
    let (raw, len) = SockAddrInternal::new(addr);
    let res = unsafe { bind(fd, raw.as_ptr(), len) };
    if res < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn close_fd(fd: RawFd) {
    unsafe {
        let _ = close(fd);
    }
}

pub struct TcpStream {
    inner: std::net::TcpStream,
}

impl TcpStream {
    pub fn connect(addr: SocketAddr) -> io::Result<(Self, bool)> {
        let domain = match addr {
            SocketAddr::V4(_) => AF_INET,
            SocketAddr::V6(_) => AF_INET6,
        };
        let fd = create_socket(domain, SOCK_STREAM, IPPROTO_TCP)?;
        let (raw, len) = SockAddrInternal::new(addr);
        let connected = loop {
            let res = unsafe { connect(fd, raw.as_ptr(), len) };
            if res == 0 {
                break true;
            }
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(EINPROGRESS) {
                break false;
            }
            match err.kind() {
                ErrorKind::Interrupted => continue,
                ErrorKind::WouldBlock => break false,
                _ => {
                    close_fd(fd);
                    return Err(err);
                }
            }
        };

        // SAFETY: fd was created by `socket` and is owned exclusively here.
        let stream = unsafe { std::net::TcpStream::from_raw_fd(fd) };
        stream.set_nonblocking(true)?;
        Ok((TcpStream { inner: stream }, connected))
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.inner.take_error()
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.peer_addr()
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.inner.shutdown(how)
    }

    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        self.inner.set_nodelay(nodelay)
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.inner.set_ttl(ttl)
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.inner.set_read_timeout(dur)
    }

    pub fn set_write_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.inner.set_write_timeout(dur)
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl AsRawFd for TcpStream {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

pub struct TcpListener {
    inner: std::net::TcpListener,
}

impl TcpListener {
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        let domain = match addr {
            SocketAddr::V4(_) => AF_INET,
            SocketAddr::V6(_) => AF_INET6,
        };
        let fd = create_socket(domain, SOCK_STREAM, IPPROTO_TCP)?;
        if let Err(err) = set_reuseaddr(fd) {
            close_fd(fd);
            return Err(err);
        }
        if let Err(err) = bind_socket(fd, addr) {
            close_fd(fd);
            return Err(err);
        }
        let res = unsafe { listen(fd, 128) };
        if res < 0 {
            let err = io::Error::last_os_error();
            close_fd(fd);
            return Err(err);
        }
        let listener = unsafe { std::net::TcpListener::from_raw_fd(fd) };
        listener.set_nonblocking(true)?;
        Ok(Self { inner: listener })
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        match self.inner.accept() {
            Ok((stream, addr)) => {
                stream.set_nonblocking(true)?;
                Ok((TcpStream { inner: stream }, addr))
            }
            Err(err) => Err(err),
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}

impl AsRawFd for TcpListener {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

pub struct UdpSocket {
    inner: std::net::UdpSocket,
}

impl UdpSocket {
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        let domain = match addr {
            SocketAddr::V4(_) => AF_INET,
            SocketAddr::V6(_) => AF_INET6,
        };
        let fd = create_socket(domain, SOCK_DGRAM, IPPROTO_UDP)?;
        if let Err(err) = set_reuseaddr(fd) {
            close_fd(fd);
            return Err(err);
        }
        if let Err(err) = bind_socket(fd, addr) {
            close_fd(fd);
            return Err(err);
        }
        let socket = unsafe { std::net::UdpSocket::from_raw_fd(fd) };
        socket.set_nonblocking(true)?;
        Ok(Self { inner: socket })
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.inner.recv_from(buf)
    }

    pub fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        self.inner.send_to(buf, addr)
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    pub fn set_broadcast(&self, broadcast: bool) -> io::Result<()> {
        self.inner.set_broadcast(broadcast)
    }

    pub fn set_multicast_ttl_v4(&self, ttl: u32) -> io::Result<()> {
        self.inner.set_multicast_ttl_v4(ttl)
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.inner.set_read_timeout(dur)
    }

    pub fn set_write_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.inner.set_write_timeout(dur)
    }
}

impl AsRawFd for UdpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

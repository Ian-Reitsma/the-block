#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::mem::{size_of, MaybeUninit};
use std::net::{Shutdown, SocketAddr};
use std::os::windows::io::{AsRawSocket, FromRawSocket, RawSocket};
use std::sync::Once;
use std::time::Duration;

const AF_INET: i32 = 2;
const AF_INET6: i32 = 23;
const SOCK_STREAM: i32 = 1;
const SOCK_DGRAM: i32 = 2;
const IPPROTO_TCP: i32 = 6;
const IPPROTO_UDP: i32 = 17;
const SOL_SOCKET: i32 = 0xFFFF;
const SO_REUSEADDR: i32 = 0x0004;
const WSA_FLAG_OVERLAPPED: u32 = 0x0000_0001;
const SOMAXCONN: i32 = 128;

const SOCKET_ERROR: i32 = -1;
const INVALID_SOCKET: RawSocket = !0;

const FIONBIO: u32 = 0x8004_666e;

const WSAEINTR: i32 = 10004;
const WSAEINPROGRESS: i32 = 10036;
const WSAEALREADY: i32 = 10037;
const WSAEWOULDBLOCK: i32 = 10035;

#[repr(C)]
struct SockAddr {
    sa_family: u16,
    sa_data: [u8; 14],
}

#[repr(C)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: InAddr,
    sin_zero: [u8; 8],
}

#[repr(C)]
struct InAddr {
    s_addr: u32,
}

#[repr(C)]
struct SockAddrIn6 {
    sin6_family: u16,
    sin6_port: u16,
    sin6_flowinfo: u32,
    sin6_addr: In6Addr,
    sin6_scope_id: u32,
}

#[repr(C)]
struct In6Addr {
    s6_addr: [u8; 16],
}

enum SockAddrInternal {
    V4(SockAddrIn),
    V6(SockAddrIn6),
}

impl SockAddrInternal {
    fn new(addr: SocketAddr) -> (Self, i32) {
        match addr {
            SocketAddr::V4(v4) => {
                let sin = SockAddrIn {
                    sin_family: AF_INET as u16,
                    sin_port: v4.port().to_be(),
                    sin_addr: InAddr {
                        s_addr: u32::from_ne_bytes(v4.ip().octets()),
                    },
                    sin_zero: [0; 8],
                };
                (Self::V4(sin), size_of::<SockAddrIn>() as i32)
            }
            SocketAddr::V6(v6) => {
                let sin6 = SockAddrIn6 {
                    sin6_family: AF_INET6 as u16,
                    sin6_port: v6.port().to_be(),
                    sin6_flowinfo: v6.flowinfo(),
                    sin6_addr: In6Addr {
                        s6_addr: v6.ip().octets(),
                    },
                    sin6_scope_id: v6.scope_id(),
                };
                (Self::V6(sin6), size_of::<SockAddrIn6>() as i32)
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

#[repr(C)]
struct Wsadata {
    data: [u8; 400],
}

extern "system" {
    fn WSAStartup(version: u16, data: *mut Wsadata) -> i32;
    fn WSAGetLastError() -> i32;
    fn WSASocketW(
        af: i32,
        kind: i32,
        protocol: i32,
        protocol_info: *mut c_void,
        group: u32,
        flags: u32,
    ) -> RawSocket;
    fn closesocket(socket: RawSocket) -> i32;
    fn ioctlsocket(socket: RawSocket, cmd: u32, argp: *mut u32) -> i32;
    fn bind(socket: RawSocket, name: *const SockAddr, namelen: i32) -> i32;
    fn connect(socket: RawSocket, name: *const SockAddr, namelen: i32) -> i32;
    fn listen(socket: RawSocket, backlog: i32) -> i32;
    fn setsockopt(
        socket: RawSocket,
        level: i32,
        optname: i32,
        optval: *const i32,
        optlen: i32,
    ) -> i32;
}

fn ensure_wsa_started() -> io::Result<()> {
    static START: Once = Once::new();
    static mut INIT_RESULT: i32 = 0;
    START.call_once(|| unsafe {
        let mut data = MaybeUninit::<Wsadata>::uninit();
        INIT_RESULT = WSAStartup(0x0202, data.as_mut_ptr());
    });
    let code = unsafe { INIT_RESULT };
    if code != 0 {
        Err(io::Error::from_raw_os_error(code))
    } else {
        Ok(())
    }
}

fn last_wsa_error() -> io::Error {
    let code = unsafe { WSAGetLastError() };
    io::Error::from_raw_os_error(code)
}

fn last_wsa_error_code() -> i32 {
    unsafe { WSAGetLastError() }
}

fn create_socket(domain: i32, ty: i32, protocol: i32) -> io::Result<RawSocket> {
    ensure_wsa_started()?;
    let socket = unsafe {
        WSASocketW(
            domain,
            ty,
            protocol,
            std::ptr::null_mut(),
            0,
            WSA_FLAG_OVERLAPPED,
        )
    };
    if socket == INVALID_SOCKET {
        return Err(last_wsa_error());
    }
    let mut nonblocking: u32 = 1;
    let res = unsafe { ioctlsocket(socket, FIONBIO, &mut nonblocking) };
    if res == SOCKET_ERROR {
        let err = last_wsa_error();
        unsafe {
            closesocket(socket);
        }
        return Err(err);
    }
    Ok(socket)
}

fn set_reuseaddr(socket: RawSocket) -> io::Result<()> {
    let value: i32 = 1;
    let res = unsafe {
        setsockopt(
            socket,
            SOL_SOCKET,
            SO_REUSEADDR,
            &value,
            size_of::<i32>() as i32,
        )
    };
    if res == SOCKET_ERROR {
        Err(last_wsa_error())
    } else {
        Ok(())
    }
}

fn bind_socket(socket: RawSocket, addr: SocketAddr) -> io::Result<()> {
    let (raw, len) = SockAddrInternal::new(addr);
    let res = unsafe { bind(socket, raw.as_ptr(), len) };
    if res == SOCKET_ERROR {
        Err(last_wsa_error())
    } else {
        Ok(())
    }
}

fn close_socket(socket: RawSocket) {
    unsafe {
        let _ = closesocket(socket);
    }
}

pub struct TcpStream {
    inner: std::net::TcpStream,
}

impl TcpStream {
    pub fn connect(addr: SocketAddr) -> io::Result<(Self, bool)> {
        ensure_wsa_started()?;
        let domain = match addr {
            SocketAddr::V4(_) => AF_INET,
            SocketAddr::V6(_) => AF_INET6,
        };
        let socket = create_socket(domain, SOCK_STREAM, IPPROTO_TCP)?;
        let (raw, len) = SockAddrInternal::new(addr);
        let connected = loop {
            let res = unsafe { connect(socket, raw.as_ptr(), len) };
            if res == 0 {
                break true;
            }
            let code = last_wsa_error_code();
            match code {
                WSAEINPROGRESS | WSAEWOULDBLOCK | WSAEALREADY => break false,
                WSAEINTR => continue,
                _ => {
                    close_socket(socket);
                    return Err(io::Error::from_raw_os_error(code));
                }
            }
        };

        let stream = unsafe { std::net::TcpStream::from_raw_socket(socket) };
        stream.set_nonblocking(true)?;
        Ok((Self { inner: stream }, connected))
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

impl AsRawSocket for TcpStream {
    fn as_raw_socket(&self) -> RawSocket {
        self.inner.as_raw_socket()
    }
}

impl AsRawSocket for TcpListener {
    fn as_raw_socket(&self) -> RawSocket {
        self.inner.as_raw_socket()
    }
}

impl AsRawSocket for UdpSocket {
    fn as_raw_socket(&self) -> RawSocket {
        self.inner.as_raw_socket()
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

pub struct TcpListener {
    inner: std::net::TcpListener,
}

impl TcpListener {
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        ensure_wsa_started()?;
        let domain = match addr {
            SocketAddr::V4(_) => AF_INET,
            SocketAddr::V6(_) => AF_INET6,
        };
        let socket = create_socket(domain, SOCK_STREAM, IPPROTO_TCP)?;
        set_reuseaddr(socket)?;
        if let Err(err) = bind_socket(socket, addr) {
            close_socket(socket);
            return Err(err);
        }
        let res = unsafe { listen(socket, SOMAXCONN) };
        if res == SOCKET_ERROR {
            let err = last_wsa_error();
            close_socket(socket);
            return Err(err);
        }
        let listener = unsafe { std::net::TcpListener::from_raw_socket(socket) };
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

pub struct UdpSocket {
    inner: std::net::UdpSocket,
}

impl UdpSocket {
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        ensure_wsa_started()?;
        let domain = match addr {
            SocketAddr::V4(_) => AF_INET,
            SocketAddr::V6(_) => AF_INET6,
        };
        let socket = create_socket(domain, SOCK_DGRAM, IPPROTO_UDP)?;
        set_reuseaddr(socket)?;
        if let Err(err) = bind_socket(socket, addr) {
            close_socket(socket);
            return Err(err);
        }
        let udp = unsafe { std::net::UdpSocket::from_raw_socket(socket) };
        udp.set_nonblocking(true)?;
        Ok(Self { inner: udp })
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

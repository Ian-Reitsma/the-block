use std::io;
use std::net::SocketAddr;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{TcpListener, TcpStream, UdpSocket};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{TcpListener, TcpStream, UdpSocket};

#[cfg(not(any(unix, target_os = "windows")))]
mod unsupported;
#[cfg(not(any(unix, target_os = "windows")))]
pub use unsupported::{TcpListener, TcpStream, UdpSocket};

/// Attempts to establish a TCP connection in non-blocking mode, returning a
/// stream wrapper alongside metadata describing whether the handshake
/// completed immediately.
pub fn connect(addr: SocketAddr) -> io::Result<(TcpStream, bool)> {
    TcpStream::connect(addr)
}

/// Binds a TCP listener in non-blocking mode.
pub fn bind_tcp_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Binds a UDP socket in non-blocking mode.
pub fn bind_udp_socket(addr: SocketAddr) -> io::Result<UdpSocket> {
    UdpSocket::bind(addr)
}

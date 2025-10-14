use std::future::Future;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(target_os = "windows")]
use std::os::windows::io::AsRawSocket;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use super::{InHouseRuntime, IoRegistration, ReactorInner};
use sys::net::{
    self, TcpListener as SysTcpListener, TcpStream as SysTcpStream, UdpSocket as SysUdpSocket,
};
use sys::reactor::Interest as ReactorInterest;

#[cfg(unix)]
fn reactor_raw_of<T: AsRawFd>(value: &T) -> super::ReactorRaw {
    value.as_raw_fd()
}

#[cfg(target_os = "windows")]
fn reactor_raw_of<T: AsRawSocket>(value: &T) -> super::ReactorRaw {
    value.as_raw_socket()
}

pub(crate) struct TcpListener {
    reactor: Arc<ReactorInner>,
    inner: Mutex<SysTcpListener>,
    registration: IoRegistration,
}

impl TcpListener {
    pub(crate) fn bind(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<Self> {
        let reactor = runtime.reactor();
        let listener = net::bind_tcp_listener(addr)?;
        let fd = reactor_raw_of(&listener);
        let registration =
            IoRegistration::new(Arc::clone(&reactor), fd, ReactorInterest::READABLE)?;
        Ok(Self {
            reactor,
            inner: Mutex::new(listener),
            registration,
        })
    }

    pub(crate) fn accept(&self) -> AcceptFuture<'_> {
        AcceptFuture { listener: self }
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        let guard = self
            .inner
            .lock()
            .expect("inhouse tcp listener mutex poisoned");
        guard.local_addr()
    }
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        drop(self.inner.lock());
        let _ = self.registration.deregister();
    }
}

pub(crate) struct TcpStream {
    inner: SysTcpStream,
    registration: IoRegistration,
}

impl TcpStream {
    fn new(reactor: Arc<ReactorInner>, stream: SysTcpStream) -> io::Result<Self> {
        let fd = reactor_raw_of(&stream);
        let registration = IoRegistration::new(
            reactor,
            fd,
            ReactorInterest::READABLE | ReactorInterest::WRITABLE,
        )?;
        Ok(Self {
            inner: stream,
            registration,
        })
    }

    pub(crate) fn connect(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<ConnectFuture> {
        let reactor = runtime.reactor();
        let (stream, ready) = net::connect(addr)?;
        let fd = reactor_raw_of(&stream);
        let registration = IoRegistration::new(
            Arc::clone(&reactor),
            fd,
            ReactorInterest::READABLE | ReactorInterest::WRITABLE,
        )?;
        Ok(ConnectFuture {
            stream: Some(stream),
            registration: Some(registration),
            ready,
        })
    }

    pub(crate) fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> TcpReadFuture<'a> {
        TcpReadFuture { stream: self, buf }
    }

    pub(crate) fn write<'a>(&'a mut self, buf: &'a [u8]) -> TcpWriteFuture<'a> {
        TcpWriteFuture { stream: self, buf }
    }

    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        // TCP sockets do not expose a userspace flush mechanism; writes are
        // forwarded to the kernel immediately. Treat flush as a no-op so
        // callers can rely on consistent semantics across backends without
        // risking hangs waiting for spurious write readiness events.
        Ok(())
    }

    pub(crate) async fn shutdown(&mut self) -> io::Result<()> {
        self.inner.shutdown(Shutdown::Both)
    }

    pub(crate) fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.peer_addr()
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let _ = self.registration.deregister();
    }
}

pub(crate) struct UdpSocket {
    inner: SysUdpSocket,
    registration: IoRegistration,
}

impl UdpSocket {
    pub(crate) fn bind(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<Self> {
        let reactor = runtime.reactor();
        let udp = net::bind_udp_socket(addr)?;
        let fd = reactor_raw_of(&udp);
        let registration = IoRegistration::new(
            Arc::clone(&reactor),
            fd,
            ReactorInterest::READABLE | ReactorInterest::WRITABLE,
        )?;
        Ok(Self {
            inner: udp,
            registration,
        })
    }

    pub(crate) fn recv_from<'a>(&'a mut self, buf: &'a mut [u8]) -> UdpRecvFuture<'a> {
        UdpRecvFuture { socket: self, buf }
    }

    pub(crate) fn send_to<'a>(&'a mut self, buf: &'a [u8], addr: SocketAddr) -> UdpSendFuture<'a> {
        UdpSendFuture {
            socket: self,
            buf,
            addr,
        }
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        let _ = self.registration.deregister();
    }
}

pub(crate) struct AcceptFuture<'a> {
    listener: &'a TcpListener,
}

impl<'a> Future for AcceptFuture<'a> {
    type Output = io::Result<(TcpStream, SocketAddr)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let guard = self
                .listener
                .inner
                .lock()
                .expect("inhouse tcp listener mutex poisoned");
            match guard.accept() {
                Ok((stream, addr)) => {
                    drop(guard);
                    let tcp_stream = TcpStream::new(Arc::clone(&self.listener.reactor), stream)?;
                    return Poll::Ready(Ok((tcp_stream, addr)));
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    drop(guard);
                    match self.listener.registration.poll_read_ready(cx) {
                        Poll::Ready(Ok(())) => continue,
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Err(err) => {
                    drop(guard);
                    return Poll::Ready(Err(err));
                }
            }
        }
    }
}

pub(crate) struct ConnectFuture {
    stream: Option<SysTcpStream>,
    registration: Option<IoRegistration>,
    ready: bool,
}

impl Future for ConnectFuture {
    type Output = io::Result<TcpStream>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            if this.ready {
                let stream = this.stream.as_mut().expect("connect future missing stream");
                match stream.take_error()? {
                    Some(err) => {
                        if err.kind() == ErrorKind::WouldBlock || is_connect_in_progress(&err) {
                            this.ready = false;
                            continue;
                        }
                        return Poll::Ready(Err(err));
                    }
                    None => {
                        let stream = this.stream.take().expect("stream taken");
                        let registration = this.registration.take().expect("registration missing");
                        let tcp_stream = TcpStream {
                            inner: stream,
                            registration,
                        };
                        return Poll::Ready(Ok(tcp_stream));
                    }
                }
            }

            let registration = this
                .registration
                .as_ref()
                .expect("connect future registration missing");
            match registration.poll_write_ready(cx) {
                Poll::Ready(Ok(())) => {
                    this.ready = true;
                    continue;
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

fn is_connect_in_progress(err: &io::Error) -> bool {
    err.raw_os_error()
        .map(is_connect_in_progress_code)
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_connect_in_progress_code(code: i32) -> bool {
    matches!(code, 115 | 114 | 36 | 37)
}

#[cfg(windows)]
fn is_connect_in_progress_code(code: i32) -> bool {
    matches!(code, 10035 | 10036 | 10037)
}

#[cfg(not(any(unix, windows)))]
fn is_connect_in_progress_code(_code: i32) -> bool {
    false
}

pub(crate) struct TcpReadFuture<'a> {
    stream: &'a mut TcpStream,
    buf: &'a mut [u8],
}

impl<'a> Future for TcpReadFuture<'a> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.stream.inner.read(this.buf) {
                Ok(bytes) => return Poll::Ready(Ok(bytes)),
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    match this.stream.registration.poll_read_ready(cx) {
                        Poll::Ready(Ok(())) => continue,
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }
}

pub(crate) struct TcpWriteFuture<'a> {
    stream: &'a mut TcpStream,
    buf: &'a [u8],
}

impl<'a> Future for TcpWriteFuture<'a> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.stream.inner.write(this.buf) {
                Ok(bytes) => return Poll::Ready(Ok(bytes)),
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    match this.stream.registration.poll_write_ready(cx) {
                        Poll::Ready(Ok(())) => continue,
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }
}

pub(crate) struct UdpRecvFuture<'a> {
    socket: &'a mut UdpSocket,
    buf: &'a mut [u8],
}

impl<'a> Future for UdpRecvFuture<'a> {
    type Output = io::Result<(usize, SocketAddr)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.socket.inner.recv_from(this.buf) {
                Ok((bytes, addr)) => return Poll::Ready(Ok((bytes, addr))),
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    match this.socket.registration.poll_read_ready(cx) {
                        Poll::Ready(Ok(())) => continue,
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }
}

pub(crate) struct UdpSendFuture<'a> {
    socket: &'a mut UdpSocket,
    buf: &'a [u8],
    addr: SocketAddr,
}

impl<'a> Future for UdpSendFuture<'a> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.socket.inner.send_to(this.buf, this.addr) {
                Ok(bytes) => return Poll::Ready(Ok(bytes)),
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    match this.socket.registration.poll_write_ready(cx) {
                        Poll::Ready(Ok(())) => continue,
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }
}

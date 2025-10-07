use std::future::Future;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use mio::net::{
    TcpListener as MioTcpListener, TcpStream as MioTcpStream, UdpSocket as MioUdpSocket,
};
use mio::Interest;
use socket2::{Domain, Protocol, Socket, Type};

use super::{InHouseRuntime, IoRegistration, ReactorInner};

pub(crate) struct TcpListener {
    reactor: Arc<ReactorInner>,
    inner: Mutex<MioTcpListener>,
    registration: IoRegistration,
}

impl TcpListener {
    pub(crate) fn bind(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<Self> {
        let reactor = runtime.reactor();
        let socket = create_tcp_socket(addr)?;
        socket.bind(&addr.into())?;
        socket.listen(1024)?;
        socket.set_nonblocking(true)?;
        let std_listener: std::net::TcpListener = socket.into();
        let mut listener = MioTcpListener::from_std(std_listener);
        let registration =
            IoRegistration::new(Arc::clone(&reactor), &mut listener, Interest::READABLE)?;
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
        if let Ok(mut inner) = self.inner.lock() {
            let _ = self.registration.deregister(&mut *inner);
        }
    }
}

pub(crate) struct TcpStream {
    inner: MioTcpStream,
    registration: IoRegistration,
}

impl TcpStream {
    fn new(reactor: Arc<ReactorInner>, mut stream: MioTcpStream) -> io::Result<Self> {
        let registration = IoRegistration::new(
            reactor,
            &mut stream,
            Interest::READABLE | Interest::WRITABLE,
        )?;
        Ok(Self {
            inner: stream,
            registration,
        })
    }

    pub(crate) fn connect(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<ConnectFuture> {
        let reactor = runtime.reactor();
        let socket = create_tcp_socket(addr)?;
        socket.set_nonblocking(true)?;
        let addr_any = &addr.into();
        let pending = match socket.connect(addr_any) {
            Ok(()) => false,
            Err(err) => {
                let in_progress = err.raw_os_error() == Some(libc::EINPROGRESS);
                if err.kind() == ErrorKind::WouldBlock || in_progress {
                    true
                } else {
                    return Err(err);
                }
            }
        };
        let std_stream: std::net::TcpStream = socket.into();
        std_stream.set_nonblocking(true)?;
        let mut stream = MioTcpStream::from_std(std_stream);
        let registration = IoRegistration::new(
            Arc::clone(&reactor),
            &mut stream,
            Interest::READABLE | Interest::WRITABLE,
        )?;
        Ok(ConnectFuture {
            stream: Some(stream),
            registration: Some(registration),
            ready: !pending,
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
        let registration = &self.registration;
        let inner = &mut self.inner;
        let _ = registration.deregister(inner);
    }
}

pub(crate) struct UdpSocket {
    inner: MioUdpSocket,
    registration: IoRegistration,
}

impl UdpSocket {
    pub(crate) fn bind(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<Self> {
        let reactor = runtime.reactor();
        let socket = create_udp_socket(addr)?;
        socket.bind(&addr.into())?;
        socket.set_nonblocking(true)?;
        let std_socket: std::net::UdpSocket = socket.into();
        let mut udp = MioUdpSocket::from_std(std_socket);
        let registration = IoRegistration::new(
            Arc::clone(&reactor),
            &mut udp,
            Interest::READABLE | Interest::WRITABLE,
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
        let registration = &self.registration;
        let inner = &mut self.inner;
        let _ = registration.deregister(inner);
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
    stream: Option<MioTcpStream>,
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
                        let in_progress = err.raw_os_error() == Some(libc::EINPROGRESS);
                        if err.kind() == ErrorKind::WouldBlock || in_progress {
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

fn create_tcp_socket(addr: SocketAddr) -> io::Result<Socket> {
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    Socket::new(domain, Type::STREAM, Some(Protocol::TCP))
}

fn create_udp_socket(addr: SocketAddr) -> io::Result<Socket> {
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))
}

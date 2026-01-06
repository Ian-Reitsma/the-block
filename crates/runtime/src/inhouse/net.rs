use std::future::Future;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener as StdTcpListener};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(target_os = "windows")]
use std::os::windows::io::AsRawSocket;
use std::pin::Pin;
use std::sync::{Arc, LockResult, Mutex};
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

fn tcp_debug_enabled() -> bool {
    std::env::var("RUNTIME_TCP_DEBUG").is_ok()
}

const READ_WITHOUT_READY_METRIC: &str = "runtime_read_without_ready_total";
const WRITE_WITHOUT_READY_METRIC: &str = "runtime_write_without_ready_total";

pub(crate) struct TcpListener {
    reactor: Arc<ReactorInner>,
    inner: Mutex<SysTcpListener>,
    registration: IoRegistration,
}

impl TcpListener {
    pub(crate) fn bind(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<Self> {
        let reactor = runtime.reactor();
        let listener = net::bind_tcp_listener(addr)?;
        if tcp_debug_enabled() {
            eprintln!(
                "[RUNTIME_TCP] bound listener fd={} addr={}",
                reactor_raw_of(&listener),
                addr
            );
        }
        let fd = reactor_raw_of(&listener);
        let registration =
            IoRegistration::new(Arc::clone(&reactor), fd, ReactorInterest::READABLE)?;
        Ok(Self {
            reactor,
            inner: Mutex::new(listener),
            registration,
        })
    }

    pub(crate) fn from_std(runtime: &InHouseRuntime, listener: SysTcpListener) -> io::Result<Self> {
        let reactor = runtime.reactor();
        let fd = reactor_raw_of(&listener);
        let registration =
            IoRegistration::new(Arc::clone(&reactor), fd, ReactorInterest::READABLE)?;
        Ok(Self {
            reactor,
            inner: Mutex::new(listener),
            registration,
        })
    }

    pub(crate) fn into_std(self) -> io::Result<StdTcpListener> {
        let cloned = {
            let guard = self.inner.lock().recover();
            guard.try_clone_std()?
        };
        cloned.set_nonblocking(false)?;
        Ok(cloned)
    }

    pub(crate) fn accept(&self) -> AcceptFuture<'_> {
        AcceptFuture { listener: self }
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        let guard = self.inner.lock().recover();
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
    pub(crate) fn new(reactor: Arc<ReactorInner>, stream: SysTcpStream) -> io::Result<Self> {
        let fd = reactor_raw_of(&stream);
        let registration = IoRegistration::new(reactor, fd, ReactorInterest::READABLE)?;
        Ok(Self {
            inner: stream,
            registration,
        })
    }

    pub(crate) fn connect(runtime: &InHouseRuntime, addr: SocketAddr) -> io::Result<ConnectFuture> {
        let reactor = runtime.reactor();
        let (stream, ready) = net::connect(addr)?;
        if tcp_debug_enabled() {
            eprintln!(
                "[RUNTIME_TCP] connect fd={} addr={} ready={}",
                reactor_raw_of(&stream),
                addr,
                ready
            );
        }
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
        TcpReadFuture {
            stream: self,
            buf,
            backoff: None,
            backoff_triggered: false,
        }
    }

    pub(crate) fn write<'a>(&'a mut self, buf: &'a [u8]) -> TcpWriteFuture<'a> {
        TcpWriteFuture {
            stream: self,
            buf,
            waiting_for_write: false,
            backoff: None,
            backoff_triggered: false,
        }
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
        let registration =
            IoRegistration::new(Arc::clone(&reactor), fd, ReactorInterest::READABLE)?;
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
            waiting_for_write: false,
            backoff: None,
            backoff_triggered: false,
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

impl Future for AcceptFuture<'_> {
    type Output = io::Result<(TcpStream, SocketAddr)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let guard = self.listener.inner.lock().recover();
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
                let Some(stream) = this.stream.as_mut() else {
                    return Poll::Ready(Err(io::Error::new(
                        ErrorKind::Other,
                        "connect future missing stream",
                    )));
                };
                match stream.take_error()? {
                    Some(err) => {
                        if err.kind() == ErrorKind::WouldBlock || is_connect_in_progress(&err) {
                            this.ready = false;
                            continue;
                        }
                        return Poll::Ready(Err(err));
                    }
                    None => {
                        let Some(stream) = this.stream.take() else {
                            return Poll::Ready(Err(io::Error::new(
                                ErrorKind::Other,
                                "connect future missing stream",
                            )));
                        };
                        let Some(registration) = this.registration.take() else {
                            return Poll::Ready(Err(io::Error::new(
                                ErrorKind::Other,
                                "connect future missing registration",
                            )));
                        };
                        if let Err(err) = registration.disable_write_interest() {
                            return Poll::Ready(Err(err));
                        }
                        let tcp_stream = TcpStream {
                            inner: stream,
                            registration,
                        };
                        return Poll::Ready(Ok(tcp_stream));
                    }
                }
            }

            let Some(registration) = this.registration.as_ref() else {
                return Poll::Ready(Err(io::Error::new(
                    ErrorKind::Other,
                    "connect future missing registration",
                )));
            };
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
    backoff: Option<super::InHouseSleep>,
    backoff_triggered: bool,
}

impl Future for TcpReadFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.stream.inner.read(this.buf) {
                Ok(bytes) => {
                    if this.backoff_triggered && bytes > 0 {
                        foundation_metrics::increment_counter!(READ_WITHOUT_READY_METRIC, 1);
                    }
                    this.backoff_triggered = false;
                    this.backoff = None;
                    if tcp_debug_enabled() {
                        eprintln!(
                            "[TCP] fd={} read {} bytes",
                            this.stream.registration.fd, bytes
                        );
                    }
                    return Poll::Ready(Ok(bytes));
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    if tcp_debug_enabled() {
                        eprintln!(
                            "[TCP] fd={} read would block; awaiting readiness",
                            this.stream.registration.fd
                        );
                    }
                    match this.stream.registration.poll_read_ready(cx) {
                        Poll::Ready(Ok(())) => {
                            this.backoff_triggered = false;
                            this.backoff = None;
                            continue;
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                        Poll::Pending => {
                            if this.backoff.is_none() {
                                // Fallback to a short sleep to avoid hanging if an IO
                                // readiness notification is missed by the reactor.
                                this.backoff = Some(super::InHouseSleep::new(
                                    Arc::clone(&this.stream.registration.reactor),
                                    super::io_read_backoff(),
                                ));
                            }
                            if let Some(backoff) = &mut this.backoff {
                                if backoff.poll(cx).is_ready() {
                                    this.backoff = None;
                                    this.backoff_triggered = true;
                                    continue;
                                }
                            }
                            return Poll::Pending;
                        }
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
    waiting_for_write: bool,
    backoff: Option<super::InHouseSleep>,
    backoff_triggered: bool,
}

impl Future for TcpWriteFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.stream.inner.write(this.buf) {
                Ok(bytes) => {
                    if this.backoff_triggered && bytes > 0 {
                        foundation_metrics::increment_counter!(WRITE_WITHOUT_READY_METRIC, 1);
                    }
                    this.backoff_triggered = false;
                    this.backoff = None;
                    if this.waiting_for_write {
                        let _ = this.stream.registration.disable_write_interest();
                        this.waiting_for_write = false;
                    }
                    if tcp_debug_enabled() {
                        eprintln!(
                            "[TCP] fd={} wrote {} bytes",
                            this.stream.registration.fd, bytes
                        );
                    }
                    return Poll::Ready(Ok(bytes));
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    if tcp_debug_enabled() {
                        eprintln!(
                            "[TCP] fd={} write would block; awaiting readiness",
                            this.stream.registration.fd
                        );
                    }
                    if !this.waiting_for_write {
                        if let Err(err) = this.stream.registration.enable_write_interest() {
                            return Poll::Ready(Err(err));
                        }
                        this.waiting_for_write = true;
                    }
                    match this.stream.registration.poll_write_ready(cx) {
                        Poll::Ready(Ok(())) => {
                            let _ = this.stream.registration.disable_write_interest();
                            this.waiting_for_write = false;
                            this.backoff_triggered = false;
                            this.backoff = None;
                            continue;
                        }
                        Poll::Ready(Err(err)) => {
                            let _ = this.stream.registration.disable_write_interest();
                            this.waiting_for_write = false;
                            this.backoff_triggered = false;
                            this.backoff = None;
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            if this.backoff.is_none() {
                                // Fallback to a short sleep to avoid hanging if an IO
                                // readiness notification is missed by the reactor.
                                this.backoff = Some(super::InHouseSleep::new(
                                    Arc::clone(&this.stream.registration.reactor),
                                    super::io_write_backoff(),
                                ));
                            }
                            if let Some(backoff) = &mut this.backoff {
                                if backoff.poll(cx).is_ready() {
                                    this.backoff = None;
                                    this.backoff_triggered = true;
                                    continue;
                                }
                            }
                            return Poll::Pending;
                        }
                    }
                }
                Err(err) => {
                    if this.waiting_for_write {
                        let _ = this.stream.registration.disable_write_interest();
                        this.waiting_for_write = false;
                    }
                    this.backoff_triggered = false;
                    this.backoff = None;
                    return Poll::Ready(Err(err));
                }
            }
        }
    }
}

pub(crate) struct UdpRecvFuture<'a> {
    socket: &'a mut UdpSocket,
    buf: &'a mut [u8],
}

impl Future for UdpRecvFuture<'_> {
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
    waiting_for_write: bool,
    backoff: Option<super::InHouseSleep>,
    backoff_triggered: bool,
}

impl Future for UdpSendFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.socket.inner.send_to(this.buf, this.addr) {
                Ok(bytes) => {
                    if this.backoff_triggered && bytes > 0 {
                        foundation_metrics::increment_counter!(WRITE_WITHOUT_READY_METRIC, 1);
                    }
                    this.backoff_triggered = false;
                    this.backoff = None;
                    return Poll::Ready(Ok(bytes));
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    if !this.waiting_for_write {
                        if let Err(err) = this.socket.registration.enable_write_interest() {
                            return Poll::Ready(Err(err));
                        }
                        this.waiting_for_write = true;
                    }
                    match this.socket.registration.poll_write_ready(cx) {
                        Poll::Ready(Ok(())) => {
                            let _ = this.socket.registration.disable_write_interest();
                            this.waiting_for_write = false;
                            this.backoff_triggered = false;
                            this.backoff = None;
                            continue;
                        }
                        Poll::Ready(Err(err)) => {
                            let _ = this.socket.registration.disable_write_interest();
                            this.waiting_for_write = false;
                            this.backoff_triggered = false;
                            this.backoff = None;
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            if this.backoff.is_none() {
                                this.backoff = Some(super::InHouseSleep::new(
                                    Arc::clone(&this.socket.registration.reactor),
                                    super::io_write_backoff(),
                                ));
                            }
                            if let Some(backoff) = &mut this.backoff {
                                if backoff.poll(cx).is_ready() {
                                    this.backoff = None;
                                    this.backoff_triggered = true;
                                    continue;
                                }
                            }
                            return Poll::Pending;
                        }
                    }
                }
                Err(err) => {
                    if this.waiting_for_write {
                        let _ = this.socket.registration.disable_write_interest();
                        this.waiting_for_write = false;
                    }
                    this.backoff_triggered = false;
                    this.backoff = None;
                    return Poll::Ready(Err(err));
                }
            }
        }
    }
}
trait LockResultExt<T> {
    fn recover(self) -> T;
}

impl<T> LockResultExt<T> for LockResult<T> {
    fn recover(self) -> T {
        match self {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

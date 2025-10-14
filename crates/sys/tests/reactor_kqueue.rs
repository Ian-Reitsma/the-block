#![cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]

use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use sys::net::{bind_tcp_listener, connect};
use sys::reactor::{Events, Interest, Poll, Token};

#[test]
fn kqueue_reports_listener_activity_and_stream_io() -> io::Result<()> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let listener = bind_tcp_listener(addr)?;
    let local_addr = listener.local_addr()?;

    let poll = Poll::new()?;
    poll.register(listener.as_raw_fd(), Token(0), Interest::READABLE)?;
    let waker = poll.create_waker(Token(1))?;

    let mut events = Events::with_capacity(8);

    waker.wake()?;
    poll.poll(&mut events, Some(Duration::from_millis(50)))?;
    assert!(events.iter().any(|event| event.token() == Token(1)));

    let (mut client_stream, _immediate) = connect(local_addr)?;

    let mut accepted_stream = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        poll.poll(&mut events, Some(Duration::from_millis(20)))?;
        if events
            .iter()
            .any(|event| event.token() == Token(0) && event.is_readable())
        {
            match listener.accept() {
                Ok((stream, _)) => {
                    accepted_stream = Some(stream);
                    break;
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => continue,
                Err(err) => return Err(err),
            }
        }
    }

    let mut server_stream = accepted_stream.ok_or_else(|| {
        io::Error::new(
            ErrorKind::TimedOut,
            "listener never produced an accepted connection",
        )
    })?;

    poll.register(
        server_stream.as_raw_fd(),
        Token(2),
        Interest::READABLE | Interest::WRITABLE,
    )?;
    poll.register(
        client_stream.as_raw_fd(),
        Token(3),
        Interest::READABLE | Interest::WRITABLE,
    )?;

    let payload = b"first-party";
    let mut wrote = false;
    let mut received = false;
    let deadline = Instant::now() + Duration::from_secs(2);

    while Instant::now() < deadline {
        poll.poll(&mut events, Some(Duration::from_millis(20)))?;
        for event in events.iter() {
            match event.token() {
                Token(2) if event.is_writable() && !wrote => match server_stream.write(payload) {
                    Ok(n) if n == payload.len() => wrote = true,
                    Ok(_) => return Err(io::Error::new(ErrorKind::WriteZero, "short write")),
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {}
                    Err(err) => return Err(err),
                },
                Token(3) if event.is_readable() && wrote && !received => {
                    let mut buf = [0u8; 32];
                    match client_stream.read(&mut buf) {
                        Ok(n) if n == payload.len() && &buf[..n] == payload => {
                            received = true;
                        }
                        Ok(_) => {
                            return Err(io::Error::new(
                                ErrorKind::UnexpectedEof,
                                "unexpected payload",
                            ))
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {}
                        Err(err) => return Err(err),
                    }
                }
                _ => {}
            }
        }
        if wrote && received {
            break;
        }
    }

    if !wrote {
        return Err(io::Error::new(
            ErrorKind::TimedOut,
            "never observed writable event for server stream",
        ));
    }
    if !received {
        return Err(io::Error::new(
            ErrorKind::TimedOut,
            "client failed to receive payload",
        ));
    }

    Ok(())
}

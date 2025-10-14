#![cfg(unix)]

use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr};
use std::thread;
use std::time::{Duration, Instant};

use sys::net::{bind_tcp_listener, connect};

#[test]
fn tcp_nonblocking_round_trip_stress() -> io::Result<()> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let listener = bind_tcp_listener(addr)?;
    let local_addr = listener.local_addr()?;

    // Ensure the listener surface is non-blocking before any clients connect.
    match listener.accept() {
        Err(err) if err.kind() == ErrorKind::WouldBlock => {}
        Err(err) => return Err(err),
        Ok(_) => {
            return Err(io::Error::new(
                ErrorKind::Other,
                "listener unexpectedly accepted connection without a client",
            ))
        }
    }

    for i in 0..32 {
        let (mut client, _immediate) = connect(local_addr)
            .map_err(|err| io::Error::new(err.kind(), format!("connect iteration {i}: {err}")))?;
        let mut server = wait_for_accept(&listener, Duration::from_secs(2))
            .map_err(|err| io::Error::new(err.kind(), format!("accept iteration {i}: {err}")))?;

        let payload = format!("frame-{i:02}").into_bytes();
        write_all_nonblocking(&mut client, &payload, Duration::from_secs(2))?;
        let mut inbound = vec![0u8; payload.len()];
        read_exact_nonblocking(&mut server, &mut inbound, Duration::from_secs(2))?;
        assert_eq!(payload, inbound);

        let response = format!("ack-{i:02}").into_bytes();
        write_all_nonblocking(&mut server, &response, Duration::from_secs(2))?;
        let mut outbound = vec![0u8; response.len()];
        read_exact_nonblocking(&mut client, &mut outbound, Duration::from_secs(2))?;
        assert_eq!(response, outbound);

        let _ = client.shutdown(Shutdown::Both);
        let _ = server.shutdown(Shutdown::Both);
    }

    Ok(())
}

fn wait_for_accept(
    listener: &sys::net::TcpListener,
    timeout: Duration,
) -> io::Result<sys::net::TcpStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        ErrorKind::TimedOut,
                        "timed out waiting for accept",
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err),
        }
    }
}

fn write_all_nonblocking(
    stream: &mut sys::net::TcpStream,
    data: &[u8],
    timeout: Duration,
) -> io::Result<()> {
    let mut written = 0usize;
    let deadline = Instant::now() + timeout;
    while written < data.len() {
        match stream.write(&data[written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "write returned zero bytes",
                ))
            }
            Ok(n) => written += n,
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        ErrorKind::TimedOut,
                        "timed out writing to stream",
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn read_exact_nonblocking(
    stream: &mut sys::net::TcpStream,
    buf: &mut [u8],
    timeout: Duration,
) -> io::Result<()> {
    let mut read = 0usize;
    let deadline = Instant::now() + timeout;
    while read < buf.len() {
        match stream.read(&mut buf[read..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "connection closed before buffer filled",
                ))
            }
            Ok(n) => read += n,
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        ErrorKind::TimedOut,
                        "timed out reading from stream",
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

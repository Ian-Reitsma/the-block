#![cfg(any(unix, target_os = "windows"))]

use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::thread;
use std::time::{Duration, Instant};

use sys::net::{bind_udp_socket, UdpSocket};

#[test]
fn udp_bidirectional_stress() -> io::Result<()> {
    let base_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let server = bind_udp_socket(base_addr)?;
    let server_addr = server.local_addr()?;

    let client = bind_udp_socket(base_addr)?;
    let client_addr = client.local_addr()?;

    for i in 0..128 {
        let payload = format!("msg-{i:03}").into_bytes();
        client.send_to(&payload, server_addr)?;

        let mut recv_buf = [0u8; 64];
        let (len, src) = recv_with_timeout(&server, &mut recv_buf, Duration::from_secs(1))
            .map_err(|err| io::Error::new(err.kind(), format!("recv iteration {i}: {err}")))?;
        assert_eq!(src, client_addr, "unexpected sender for iteration {i}");
        assert_eq!(&recv_buf[..len], payload.as_slice());

        let ack = format!("ack-{i:03}").into_bytes();
        server.send_to(&ack, client_addr)?;

        let (ack_len, ack_src) = recv_with_timeout(&client, &mut recv_buf, Duration::from_secs(1))
            .map_err(|err| io::Error::new(err.kind(), format!("ack iteration {i}: {err}")))?;
        assert_eq!(
            ack_src, server_addr,
            "unexpected ack source for iteration {i}"
        );
        assert_eq!(&recv_buf[..ack_len], ack.as_slice());
    }

    Ok(())
}

fn recv_with_timeout(
    socket: &UdpSocket,
    buf: &mut [u8],
    timeout: Duration,
) -> io::Result<(usize, SocketAddr)> {
    let deadline = Instant::now() + timeout;
    loop {
        match socket.recv_from(buf) {
            Ok(result) => return Ok(result),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        ErrorKind::TimedOut,
                        "timed out waiting for datagram",
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err),
        }
    }
}

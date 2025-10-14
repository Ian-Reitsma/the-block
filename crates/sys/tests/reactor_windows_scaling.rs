#![cfg(target_os = "windows")]

use std::net::UdpSocket;
use std::os::windows::io::AsRawSocket;
use std::time::Duration;

use sys::reactor::{Interest, Poll, Token};

#[test]
fn registers_more_than_sixty_four_sockets() {
    const TARGET: usize = 96;
    let poll = Poll::new().expect("create poll");
    let mut events = sys::reactor::Events::with_capacity(TARGET);
    let mut sockets = Vec::with_capacity(TARGET);

    for idx in 0..TARGET {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind udp socket");
        poll.register(
            socket.as_raw_socket(),
            Token(idx),
            Interest::READABLE | Interest::WRITABLE,
        )
        .unwrap();
        sockets.push(socket);
    }

    poll.poll(&mut events, Some(Duration::from_millis(0)))
        .unwrap();
    assert!(events.iter().count() <= TARGET);
}

use std::io;
use std::ops::{BitOr, BitOrAssign};
use std::time::Duration;

#[cfg(unix)]
use std::os::fd::RawFd;
#[cfg(target_os = "windows")]
type RawFd = std::os::windows::io::RawSocket;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interest {
    bits: u8,
}

impl Interest {
    pub const READABLE: Self = Self { bits: 0b01 };
    pub const WRITABLE: Self = Self { bits: 0b10 };

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }
}

impl BitOr for Interest {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            bits: self.bits | rhs.bits,
        }
    }
}

impl BitOrAssign for Interest {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    token: Token,
    readable: bool,
    writable: bool,
    error: bool,
    read_closed: bool,
    write_closed: bool,
    priority: bool,
}

impl Event {
    fn new(
        token: Token,
        readable: bool,
        writable: bool,
        error: bool,
        read_closed: bool,
        write_closed: bool,
        priority: bool,
    ) -> Self {
        Self {
            token,
            readable,
            writable,
            error,
            read_closed,
            write_closed,
            priority,
        }
    }

    pub fn token(&self) -> Token {
        self.token
    }

    pub fn is_readable(&self) -> bool {
        self.readable
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    pub fn is_error(&self) -> bool {
        self.error
    }

    pub fn is_read_closed(&self) -> bool {
        self.read_closed
    }

    pub fn is_write_closed(&self) -> bool {
        self.write_closed
    }

    pub fn is_priority(&self) -> bool {
        self.priority
    }
}

pub struct Events {
    inner: platform::Events,
}

impl Events {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: platform::Events::with_capacity(capacity),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.inner.iter()
    }

    pub(crate) fn as_mut_platform(&mut self) -> &mut platform::Events {
        &mut self.inner
    }
}

pub struct Poll {
    inner: platform::Poll,
}

impl Poll {
    pub fn new() -> io::Result<Self> {
        platform::Poll::new().map(|inner| Self { inner })
    }

    pub fn poll(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.poll(events.as_mut_platform(), timeout)
    }

    pub fn register(&self, fd: RawFd, token: Token, interest: Interest) -> io::Result<()> {
        self.inner.register(fd, token, interest)
    }

    pub fn deregister(&self, fd: RawFd, token: Token) -> io::Result<()> {
        self.inner.deregister(fd, token)
    }

    pub fn create_waker(&self, token: Token) -> io::Result<Waker> {
        self.inner.create_waker(token)
    }
}

pub struct Waker {
    inner: platform::Waker,
}

impl Waker {
    pub fn wake(&self) -> io::Result<()> {
        self.inner.wake()
    }
}

#[cfg(target_os = "linux")]
mod platform;
#[cfg(target_os = "linux")]
pub use platform::Waker as PlatformWaker;

#[cfg(target_os = "windows")]
mod platform_windows;
#[cfg(target_os = "windows")]
use platform_windows as platform;
#[cfg(target_os = "windows")]
pub use platform_windows::Waker as PlatformWaker;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]
mod platform_bsd;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]
use platform_bsd as platform;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]
pub use platform_bsd::Waker as PlatformWaker;

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    target_os = "windows",
)))]
mod unsupported {
    use super::{Event, Events as PublicEvents, Interest, Token};
    use std::io;
    use std::os::fd::RawFd;
    use std::time::Duration;

    pub struct Events {
        events: Vec<Event>,
    }

    impl Events {
        pub fn with_capacity(capacity: usize) -> Self {
            let _ = capacity;
            Self { events: Vec::new() }
        }

        pub fn iter(&self) -> impl Iterator<Item = &Event> {
            self.events.iter()
        }
    }

    pub struct Poll;

    impl Poll {
        pub fn new() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor polling is not yet implemented for this platform",
            ))
        }

        pub fn poll(&self, _events: &mut Events, _timeout: Option<Duration>) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor polling is not yet implemented for this platform",
            ))
        }

        pub fn register(&self, _fd: RawFd, _token: Token, _interest: Interest) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor polling is not yet implemented for this platform",
            ))
        }

        pub fn deregister(&self, _fd: RawFd, _token: Token) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor polling is not yet implemented for this platform",
            ))
        }

        pub fn create_waker(&self, _token: Token) -> io::Result<super::Waker> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor polling is not yet implemented for this platform",
            ))
        }
    }

    pub struct Waker;

    impl Waker {
        pub fn wake(&self) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor polling is not yet implemented for this platform",
            ))
        }
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    target_os = "windows",
)))]
use unsupported as platform;
#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    target_os = "windows",
)))]
pub use unsupported::Waker as PlatformWaker;

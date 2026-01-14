use super::{Event, EventFlags, Interest, Token};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::mem::size_of;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Mutex;
use std::time::Duration;

const EPOLL_CLOEXEC: i32 = 0x0008_0000;
const EPOLL_CTL_ADD: i32 = 1;
const EPOLL_CTL_DEL: i32 = 2;
const EPOLL_CTL_MOD: i32 = 3;
const EPOLLIN: u32 = 0x0000_0001;
const EPOLLPRI: u32 = 0x0000_0002;
const EPOLLOUT: u32 = 0x0000_0004;
const EPOLLERR: u32 = 0x0000_0008;
const EPOLLHUP: u32 = 0x0000_0010;
const EPOLLRDHUP: u32 = 0x0000_2000;

const EFD_CLOEXEC: i32 = 0x0008_0000;
const EFD_NONBLOCK: i32 = 0x0000_0800;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct EpollEvent {
    events: u32,
    data: u64,
}

impl EpollEvent {
    fn zeroed() -> Self {
        Self { events: 0, data: 0 }
    }
}

use std::ffi::c_void;

extern "C" {
    fn epoll_create1(flags: i32) -> i32;
    fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32;
    fn epoll_wait(epfd: i32, events: *mut EpollEvent, maxevents: i32, timeout: i32) -> i32;
    fn eventfd(initval: u32, flags: i32) -> i32;
    fn read(fd: i32, buf: *mut c_void, count: usize) -> isize;
    fn write(fd: i32, buf: *const c_void, count: usize) -> isize;
}

pub struct Poll {
    inner: Arc<Inner>,
}

use std::sync::Arc;

struct Inner {
    epoll: OwnedFd,
    wakers: Mutex<HashMap<usize, RawFd>>,
}

impl Poll {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let inner = Inner {
            epoll: unsafe { OwnedFd::from_raw_fd(fd) },
            wakers: Mutex::new(HashMap::new()),
        };
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn poll(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.poll(events, timeout)
    }

    pub fn register(&self, fd: RawFd, token: Token, interest: Interest) -> io::Result<()> {
        self.inner.register(fd, token, interest)
    }

    pub fn update_interest(
        &self,
        fd: RawFd,
        token: Token,
        _previous: Interest,
        current: Interest,
    ) -> io::Result<()> {
        self.inner.update_interest(fd, token, current)
    }

    pub fn deregister(&self, fd: RawFd, token: Token) -> io::Result<()> {
        self.inner.deregister(fd, token)
    }

    pub fn create_waker(&self, token: Token) -> io::Result<super::Waker> {
        let fd = unsafe { eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        self.inner
            .register(owned.as_raw_fd(), token, Interest::READABLE)?;
        {
            let mut map = self.inner.wakers.lock().expect("poll waker map poisoned");
            map.insert(token.0, owned.as_raw_fd());
        }
        Ok(super::Waker {
            inner: Waker {
                poll: Arc::clone(&self.inner),
                fd: owned,
                token,
            },
        })
    }
}

impl Inner {
    fn register(&self, fd: RawFd, token: Token, interest: Interest) -> io::Result<()> {
        let mut event = EpollEvent {
            events: interest_to_epoll(interest),
            data: token.0 as u64,
        };
        let res = unsafe { epoll_ctl(self.epoll.as_raw_fd(), EPOLL_CTL_ADD, fd, &mut event) };
        if res < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == ErrorKind::AlreadyExists {
                let res =
                    unsafe { epoll_ctl(self.epoll.as_raw_fd(), EPOLL_CTL_MOD, fd, &mut event) };
                if res < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            } else {
                Err(err)
            }
        } else {
            Ok(())
        }
    }

    fn update_interest(&self, fd: RawFd, token: Token, interest: Interest) -> io::Result<()> {
        self.register(fd, token, interest)
    }

    fn deregister(&self, fd: RawFd, token: Token) -> io::Result<()> {
        let mut event = EpollEvent {
            events: 0,
            data: token.0 as u64,
        };
        let res = unsafe { epoll_ctl(self.epoll.as_raw_fd(), EPOLL_CTL_DEL, fd, &mut event) };
        if res < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == ErrorKind::NotFound {
                return Ok(());
            }
            Err(err)
        } else {
            Ok(())
        }
    }

    fn poll(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        let timeout_ms = timeout
            .map(|d| d.as_millis().min(i32::MAX as u128) as i32)
            .unwrap_or(-1);
        events.events.clear();
        loop {
            let res = unsafe {
                epoll_wait(
                    self.epoll.as_raw_fd(),
                    events.storage.as_mut_ptr(),
                    events.storage.len() as i32,
                    timeout_ms,
                )
            };
            if res < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            let count = res as usize;
            for raw in events.storage.iter().take(count) {
                events.events.push(convert_event(*raw));
            }
            self.drain_wakers(&events.events);
            return Ok(());
        }
    }

    fn drain_wakers(&self, events: &[Event]) {
        for event in events {
            let fd = {
                let map = self.wakers.lock().expect("poll waker map poisoned");
                map.get(&event.token().0).copied()
            };
            if let Some(fd) = fd {
                let mut buf = [0u8; size_of::<u64>()];
                loop {
                    let res = unsafe { read(fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
                    match res.cmp(&0) {
                        Ordering::Less => {
                            let err = io::Error::last_os_error();
                            if err.kind() == ErrorKind::Interrupted {
                                continue;
                            }
                            if err.kind() == ErrorKind::WouldBlock {
                                break;
                            }
                            break;
                        }
                        Ordering::Equal => break,
                        Ordering::Greater => {
                            if (res as usize) < buf.len() {
                                break;
                            }
                            continue;
                        }
                    }
                }
            }
        }
    }
}

fn interest_to_epoll(interest: Interest) -> u32 {
    let mut flags = 0;
    if interest.contains(Interest::READABLE) {
        flags |= EPOLLIN | EPOLLRDHUP;
    }
    if interest.contains(Interest::WRITABLE) {
        flags |= EPOLLOUT;
    }
    flags
}

fn convert_event(raw: EpollEvent) -> Event {
    // Some platforms truncate or otherwise perturb the upper bits of the user
    // data field. Mask to 32 bits to keep token comparisons stable across
    // architectures while we only ever allocate tokens in the lower range.
    let token = Token((raw.data as usize) & 0xFFFF_FFFF);
    let readable = raw.events & (EPOLLIN | EPOLLRDHUP | EPOLLHUP) != 0;
    let writable = raw.events & EPOLLOUT != 0;
    let error = raw.events & EPOLLERR != 0;
    let read_closed = raw.events & (EPOLLRDHUP | EPOLLHUP) != 0;
    let write_closed = raw.events & EPOLLHUP != 0;
    let priority = raw.events & EPOLLPRI != 0;
    Event::new(
        token,
        None,
        EventFlags::new(
            readable,
            writable,
            error,
            read_closed,
            write_closed,
            priority,
        ),
    )
}

pub struct Events {
    pub(crate) storage: Vec<EpollEvent>,
    pub(crate) events: Vec<Event>,
}

impl Events {
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            storage: vec![EpollEvent::zeroed(); capacity],
            events: Vec::with_capacity(capacity),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events.iter()
    }
}

pub struct Waker {
    poll: Arc<Inner>,
    fd: OwnedFd,
    token: Token,
}

impl Waker {
    pub fn wake(&self) -> io::Result<()> {
        let value: u64 = 1;
        let bytes = value.to_ne_bytes();
        loop {
            let res = unsafe {
                write(
                    self.fd.as_raw_fd(),
                    bytes.as_ptr() as *const c_void,
                    bytes.len(),
                )
            };
            if res < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == ErrorKind::Interrupted {
                    continue;
                }
                if err.kind() == ErrorKind::WouldBlock {
                    return Ok(());
                }
                return Err(err);
            }
            return Ok(());
        }
    }
}

impl Drop for Waker {
    fn drop(&mut self) {
        let fd = self.fd.as_raw_fd();
        let _ = self.poll.deregister(fd, self.token);
        let mut map = self.poll.wakers.lock().expect("poll waker map poisoned");
        map.remove(&self.token.0);
    }
}

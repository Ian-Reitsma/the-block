#![cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]

use super::{Event, Interest, Token};
use std::ffi::c_void;
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::time::Duration;

const EV_ADD: u16 = 0x0001;
const EV_DELETE: u16 = 0x0002;
const EV_CLEAR: u16 = 0x0020;
const EV_ERROR_FLAG: u16 = 0x4000;
const EV_EOF_FLAG: u16 = 0x8000;

const EVFILT_READ: i16 = -1;
const EVFILT_WRITE: i16 = -2;
const EVFILT_USER: i16 = -10;

const NOTE_TRIGGER: u32 = 0x0100_0000;

#[repr(C)]
#[derive(Clone, Copy)]
struct KEvent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut c_void,
}

impl KEvent {
    fn zeroed() -> Self {
        Self {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: core::ptr::null_mut(),
        }
    }

    fn for_fd(fd: RawFd, filter: i16, flags: u16, token: Token) -> Self {
        Self {
            ident: fd as usize,
            filter,
            flags,
            fflags: 0,
            data: 0,
            udata: token_to_udata(token),
        }
    }

    fn delete_fd(fd: RawFd, filter: i16, token: Token) -> Self {
        Self {
            ident: fd as usize,
            filter,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: token_to_udata(token),
        }
    }

    fn add_user(token: Token) -> Self {
        Self {
            ident: token.0,
            filter: EVFILT_USER,
            flags: EV_ADD | EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: token_to_udata(token),
        }
    }

    fn delete_user(token: Token) -> Self {
        Self {
            ident: token.0,
            filter: EVFILT_USER,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: token_to_udata(token),
        }
    }

    fn trigger_user(token: Token) -> Self {
        Self {
            ident: token.0,
            filter: EVFILT_USER,
            flags: 0,
            fflags: NOTE_TRIGGER,
            data: 0,
            udata: token_to_udata(token),
        }
    }
}

fn token_to_udata(token: Token) -> *mut c_void {
    token.0 as usize as *mut c_void
}

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

extern "C" {
    fn kqueue() -> i32;
    fn kevent(
        kq: i32,
        changelist: *const KEvent,
        nchanges: i32,
        eventlist: *mut KEvent,
        nevents: i32,
        timeout: *const Timespec,
    ) -> i32;
}

pub struct Poll {
    inner: Arc<Inner>,
}

struct Inner {
    kqueue: OwnedFd,
}

impl Poll {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { kqueue() };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let inner = Inner {
            kqueue: unsafe { OwnedFd::from_raw_fd(fd) },
        };
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn poll(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.poll(events, timeout)
    }

    pub fn register(&self, fd: RawFd, token: Token, interest: Interest) -> io::Result<()> {
        self.inner.register_fd(fd, token, interest)
    }

    pub fn deregister(&self, fd: RawFd, token: Token) -> io::Result<()> {
        self.inner.deregister_fd(fd, token)
    }

    pub fn create_waker(&self, token: Token) -> io::Result<super::Waker> {
        self.inner.register_user(token)?;
        Ok(super::Waker {
            inner: Waker {
                poll: Arc::clone(&self.inner),
                token,
            },
        })
    }
}

impl Inner {
    fn poll(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        events.events.clear();
        let mut timespec = timeout.map(duration_to_timespec);
        let timeout_ptr = timespec
            .as_ref()
            .map(|ts| ts as *const Timespec)
            .unwrap_or(core::ptr::null());
        loop {
            let res = unsafe {
                kevent(
                    self.kqueue.as_raw_fd(),
                    core::ptr::null(),
                    0,
                    events.storage.as_mut_ptr(),
                    events.storage.len() as i32,
                    timeout_ptr,
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
            return Ok(());
        }
    }

    fn register_fd(&self, fd: RawFd, token: Token, interest: Interest) -> io::Result<()> {
        let mut last_err: Option<io::Error> = None;
        if interest.contains(Interest::READABLE) {
            let mut change = KEvent::for_fd(fd, EVFILT_READ, EV_ADD | EV_CLEAR, token);
            if let Err(err) = self.submit_changes(core::slice::from_mut(&mut change)) {
                last_err = Some(err);
            }
        }
        if interest.contains(Interest::WRITABLE) {
            let mut change = KEvent::for_fd(fd, EVFILT_WRITE, EV_ADD | EV_CLEAR, token);
            if let Err(err) = self.submit_changes(core::slice::from_mut(&mut change)) {
                last_err = Some(err);
            }
        }
        if let Some(err) = last_err {
            return Err(err);
        }
        Ok(())
    }

    fn deregister_fd(&self, fd: RawFd, token: Token) -> io::Result<()> {
        for filter in [EVFILT_READ, EVFILT_WRITE] {
            let mut change = KEvent::delete_fd(fd, filter, token);
            if let Err(err) = self.submit_changes(core::slice::from_mut(&mut change)) {
                if err.kind() == ErrorKind::NotFound {
                    continue;
                }
                if err.raw_os_error() == Some(ENOENT) {
                    continue;
                }
                return Err(err);
            }
        }
        Ok(())
    }

    fn register_user(&self, token: Token) -> io::Result<()> {
        let mut change = KEvent::add_user(token);
        self.submit_changes(core::slice::from_mut(&mut change))
    }

    fn delete_user(&self, token: Token) -> io::Result<()> {
        let mut change = KEvent::delete_user(token);
        match self.submit_changes(core::slice::from_mut(&mut change)) {
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) if err.raw_os_error() == Some(ENOENT) => Ok(()),
            other => other,
        }
    }

    fn trigger_user(&self, token: Token) -> io::Result<()> {
        let mut change = KEvent::trigger_user(token);
        self.submit_changes(core::slice::from_mut(&mut change))
    }

    fn submit_changes(&self, changes: &mut [KEvent]) -> io::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let res = unsafe {
            kevent(
                self.kqueue.as_raw_fd(),
                changes.as_ptr(),
                changes.len() as i32,
                core::ptr::null_mut(),
                0,
                core::ptr::null(),
            )
        };
        if res < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

fn duration_to_timespec(duration: Duration) -> Timespec {
    let secs = duration.as_secs().min(i64::MAX as u64) as i64;
    let nanos = duration.subsec_nanos() as i64;
    Timespec {
        tv_sec: secs,
        tv_nsec: nanos,
    }
}

fn convert_event(raw: KEvent) -> Event {
    let token = Token(raw.udata as usize);
    let readable = raw.filter == EVFILT_READ;
    let writable = raw.filter == EVFILT_WRITE;
    let error = (raw.flags & EV_ERROR_FLAG) != 0 && raw.data != 0;
    let read_closed = raw.filter == EVFILT_READ && (raw.flags & EV_EOF_FLAG) != 0;
    let write_closed = raw.filter == EVFILT_WRITE && (raw.flags & EV_EOF_FLAG) != 0;
    let priority = raw.filter == EVFILT_USER;
    Event::new(
        token,
        readable,
        writable,
        error,
        read_closed,
        write_closed,
        priority,
    )
}

pub struct Events {
    pub(crate) storage: Vec<KEvent>,
    pub(crate) events: Vec<Event>,
}

impl Events {
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            storage: vec![KEvent::zeroed(); capacity],
            events: Vec::with_capacity(capacity),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events.iter()
    }
}

pub struct Waker {
    poll: Arc<Inner>,
    token: Token,
}

impl Waker {
    pub fn wake(&self) -> io::Result<()> {
        self.poll.trigger_user(self.token)
    }
}

impl Drop for Waker {
    fn drop(&mut self) {
        let _ = self.poll.delete_user(self.token);
    }
}

pub use Waker as PlatformWaker;

const ENOENT: i32 = 2;

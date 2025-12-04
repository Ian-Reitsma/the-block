#![cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]

use std::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Kevent {
    pub ident: usize,
    pub filter: i16,
    pub flags: u16,
    pub fflags: u32,
    pub data: isize,
    pub udata: *mut c_void,
    pub ext: [u64; 4],
}

#[cfg(any(
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Kevent {
    pub ident: usize,
    pub filter: i16,
    pub flags: u16,
    pub fflags: u32,
    pub data: isize,
    pub udata: isize,
}

impl Kevent {
    pub(crate) fn zeroed() -> Self {
        Self {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: Self::udata_zero(),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            ext: [0; 4],
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn udata_zero() -> *mut c_void {
        std::ptr::null_mut()
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    fn udata_zero() -> isize {
        0
    }

    pub(crate) fn set_udata_null(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.udata = std::ptr::null_mut();
        }

        #[cfg(any(
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        ))]
        {
            self.udata = 0;
        }
    }
}

pub(crate) mod ffi {
    use super::{Kevent, Timespec};

    extern "C" {
        pub fn kqueue() -> i32;
        pub fn kevent(
            kq: i32,
            changelist: *const Kevent,
            nchanges: i32,
            eventlist: *mut Kevent,
            nevents: i32,
            timeout: *const Timespec,
        ) -> i32;
        pub fn close(fd: i32) -> i32;
    }
}

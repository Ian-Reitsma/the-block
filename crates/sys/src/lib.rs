pub mod error {
    use std::fmt;
    use std::io;

    #[derive(Debug)]
    pub enum SysError {
        Io(io::Error),
        Unsupported(&'static str),
    }

    impl SysError {
        pub fn unsupported(feature: &'static str) -> Self {
            Self::Unsupported(feature)
        }
    }

    impl From<io::Error> for SysError {
        fn from(value: io::Error) -> Self {
            SysError::Io(value)
        }
    }

    impl fmt::Display for SysError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                SysError::Io(err) => write!(f, "io error: {err}"),
                SysError::Unsupported(feature) => {
                    write!(f, "feature {feature} is not yet implemented")
                }
            }
        }
    }

    impl std::error::Error for SysError {}

    pub type Result<T> = std::result::Result<T, SysError>;

    impl From<SysError> for io::Error {
        fn from(value: SysError) -> Self {
            match value {
                SysError::Io(err) => err,
                SysError::Unsupported(feature) => io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("feature {feature} is not yet implemented"),
                ),
            }
        }
    }
}

#[cfg(unix)]
mod unix_ffi {
    use core::ffi::{c_int, c_uint, c_ulong, c_void};

    pub const STDIN_FILENO: c_int = 0;
    pub const STDOUT_FILENO: c_int = 1;
    pub const TIOCGWINSZ: c_ulong = 0x5413;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub const TCSANOW: c_int = 0;

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    pub const TCSANOW: c_int = 0;

    pub const SIGHUP: c_int = 1;

    pub const SA_SIGINFO: c_ulong = 0x0000_0004;
    pub const SA_RESTART: c_ulong = 0x1000_0000;

    pub const F_GETFL: c_int = 3;
    pub const F_SETFL: c_int = 4;
    pub const F_GETFD: c_int = 1;
    pub const F_SETFD: c_int = 2;
    pub const FD_CLOEXEC: c_int = 1;
    pub const O_NONBLOCK: c_int = 0o0004_000;
    pub const O_NOFOLLOW: c_int = 0o0040_0000;
    pub const LOCK_EX: c_int = 2;

    const SIGSET_WORDS: usize = 16;

    #[allow(non_camel_case_types)]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub type cc_t = u8;
    #[allow(non_camel_case_types)]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub type speed_t = c_uint;
    #[allow(non_camel_case_types)]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub type tcflag_t = c_uint;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub const NCCS: usize = 32;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub const ECHO: tcflag_t = 0x0000_0008;

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    #[allow(non_camel_case_types)]
    pub type cc_t = u8;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    #[allow(non_camel_case_types)]
    pub type speed_t = c_ulong;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    #[allow(non_camel_case_types)]
    pub type tcflag_t = c_ulong;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    pub const NCCS: usize = 20;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    pub const ECHO: tcflag_t = 0x0000_0008;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct sigset_t {
        bits: [u64; SIGSET_WORDS],
    }

    impl sigset_t {
        pub fn empty() -> Self {
            Self {
                bits: [0; SIGSET_WORDS],
            }
        }
    }

    #[repr(C)]
    pub union SigActionHandler {
        pub handler: extern "C" fn(c_int),
        pub sigaction: extern "C" fn(c_int, *mut siginfo_t, *mut c_void),
    }

    #[repr(C)]
    pub struct sigaction {
        pub sa_sigaction: SigActionHandler,
        pub sa_mask: sigset_t,
        pub sa_flags: c_ulong,
        pub sa_restorer: Option<extern "C" fn()>,
    }

    #[repr(C)]
    pub struct siginfo_t {
        _private: [u8; 128],
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct termios {
        pub c_iflag: tcflag_t,
        pub c_oflag: tcflag_t,
        pub c_cflag: tcflag_t,
        pub c_lflag: tcflag_t,
        pub c_line: cc_t,
        pub c_cc: [cc_t; NCCS],
        pub c_ispeed: speed_t,
        pub c_ospeed: speed_t,
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct termios {
        pub c_iflag: tcflag_t,
        pub c_oflag: tcflag_t,
        pub c_cflag: tcflag_t,
        pub c_lflag: tcflag_t,
        pub c_cc: [cc_t; NCCS],
        pub c_ispeed: speed_t,
        pub c_ospeed: speed_t,
    }

    #[repr(C)]
    pub struct winsize {
        pub ws_row: c_uint,
        pub ws_col: c_uint,
        pub ws_xpixel: c_uint,
        pub ws_ypixel: c_uint,
    }

    extern "C" {
        pub fn sigaction(signum: c_int, act: *const sigaction, oldact: *mut sigaction) -> c_int;
        pub fn pipe(fds: *mut c_int) -> c_int;
        pub fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
        pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        pub fn flock(fd: c_int, operation: c_int) -> c_int;
        pub fn tcgetattr(fd: c_int, termios_p: *mut termios) -> c_int;
        pub fn tcsetattr(fd: c_int, optional_actions: c_int, termios_p: *const termios) -> c_int;
        #[cfg(test)]
        pub fn raise(sig: c_int) -> c_int;
    }

    pub use self::sigaction as SigAction;
    pub use self::siginfo_t as SigInfo;
    pub use self::sigset_t as SigSet;
    pub use self::termios as Termios;
    pub use self::winsize as WinSize;
}

pub mod paths {
    use std::env;
    use std::path::PathBuf;

    /// Return the current user's home directory if known.
    pub fn home_dir() -> Option<PathBuf> {
        #[cfg(unix)]
        {
            if let Some(home) = env::var_os("HOME") {
                return Some(PathBuf::from(home));
            }
        }
        #[cfg(windows)]
        {
            if let Some(home) = env::var_os("USERPROFILE") {
                return Some(PathBuf::from(home));
            }
        }
        env::var_os("TB_HOME").map(PathBuf::from)
    }
}

pub mod cpu {
    /// Return the number of logical CPUs available to the process.
    pub fn logical_count() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    }
}

pub mod net;

pub mod reactor;

#[cfg(target_os = "linux")]
pub mod inotify {
    use std::ffi::{CString, OsString};
    use std::io::{self, ErrorKind};
    use std::mem::size_of;
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::raw::{c_char, c_int, c_void};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;

    const IN_NONBLOCK: c_int = 0o0004_000;
    const IN_CLOEXEC: c_int = 0o2000_000;

    #[repr(C)]
    struct RawInotifyEvent {
        wd: c_int,
        mask: u32,
        cookie: u32,
        len: u32,
        name: [c_char; 0],
    }

    extern "C" {
        fn inotify_init1(flags: c_int) -> c_int;
        fn inotify_add_watch(fd: c_int, name: *const c_char, mask: u32) -> c_int;
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        fn close(fd: c_int) -> c_int;
    }

    #[derive(Debug, Clone)]
    pub struct Event {
        pub watch_descriptor: i32,
        pub mask: u32,
        pub name: Option<OsString>,
    }

    #[derive(Debug)]
    pub struct Inotify {
        fd: RawFd,
        buffer: Vec<u8>,
    }

    impl Inotify {
        pub fn new() -> io::Result<Self> {
            // SAFETY: `inotify_init1` has no additional safety requirements.
            let fd = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                fd,
                buffer: vec![0u8; 64 * 1024],
            })
        }

        pub fn add_watch(&self, path: &Path, mask: u32) -> io::Result<i32> {
            let bytes = path.as_os_str().as_bytes();
            let c_path = CString::new(bytes)
                .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "path contains null byte"))?;
            // SAFETY: `inotify_add_watch` reads the provided C string.
            let wd = unsafe { inotify_add_watch(self.fd, c_path.as_ptr(), mask as u32) };
            if wd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(wd)
        }

        pub fn read_events(&mut self) -> io::Result<Vec<Event>> {
            let mut events = Vec::new();
            loop {
                // SAFETY: buffer is valid for writes.
                let read = unsafe {
                    read(
                        self.fd,
                        self.buffer.as_mut_ptr() as *mut c_void,
                        self.buffer.len(),
                    )
                };
                if read < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == ErrorKind::WouldBlock {
                        break;
                    } else if err.kind() == ErrorKind::Interrupted {
                        continue;
                    } else {
                        return Err(err);
                    }
                }
                if read == 0 {
                    break;
                }

                let mut offset = 0usize;
                while offset < read as usize {
                    // SAFETY: offset is within buffer bounds thanks to length checks.
                    let header =
                        unsafe { &*(self.buffer.as_ptr().add(offset) as *const RawInotifyEvent) };
                    let name_offset = offset + size_of::<RawInotifyEvent>();
                    let name = if header.len > 0 {
                        let name_len = (header.len.saturating_sub(1)) as usize;
                        let slice = &self.buffer[name_offset..name_offset + name_len];
                        Some(OsString::from_vec(slice.to_vec()))
                    } else {
                        None
                    };
                    events.push(Event {
                        watch_descriptor: header.wd,
                        mask: header.mask,
                        name,
                    });
                    offset += size_of::<RawInotifyEvent>() + header.len as usize;
                }
            }

            Ok(events)
        }
    }

    impl Drop for Inotify {
        fn drop(&mut self) {
            unsafe {
                let _ = close(self.fd);
            }
        }
    }

    impl AsRawFd for Inotify {
        fn as_raw_fd(&self) -> RawFd {
            self.fd
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
pub mod kqueue {
    use std::io::{self, ErrorKind};
    use std::os::fd::{AsRawFd, RawFd};

    #[derive(Debug, Clone)]
    pub struct Event {
        pub fd: RawFd,
        pub flags: u32,
    }

    #[derive(Debug)]
    pub struct Kqueue {
        fd: RawFd,
    }

    impl Kqueue {
        pub fn new() -> io::Result<Self> {
            let fd = unsafe { ffi::kqueue() };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { fd })
        }

        pub fn register(&self, ident: RawFd, flags: u32) -> io::Result<()> {
            let mut change = RawKevent::zeroed();
            change.ident = ident as usize;
            change.filter = EVFILT_VNODE;
            change.flags = (EV_ADD | EV_ENABLE | EV_CLEAR) as u16;
            change.fflags = flags;
            change.data = 0;
            change.set_udata_null();

            let res = unsafe {
                ffi::kevent(
                    self.fd,
                    &change as *const RawKevent,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if res < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn poll_raw(&self, buffer: &mut [RawKevent]) -> io::Result<usize> {
            let timeout = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            loop {
                let res = unsafe {
                    ffi::kevent(
                        self.fd,
                        std::ptr::null(),
                        0,
                        buffer.as_mut_ptr(),
                        buffer.len() as i32,
                        &timeout as *const Timespec,
                    )
                };
                if res < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(err);
                }
                return Ok(res as usize);
            }
        }

        pub fn poll_events(&self, capacity: usize) -> io::Result<Vec<Event>> {
            let mut buffer = Vec::with_capacity(capacity.max(1));
            buffer.resize_with(capacity.max(1), RawKevent::zeroed);
            let count = self.poll_raw(&mut buffer)?;
            Ok(super::kqueue::interpret_events(&buffer[..count]))
        }
    }

    impl Drop for Kqueue {
        fn drop(&mut self) {
            unsafe {
                let _ = ffi::close(self.fd);
            }
        }
    }

    impl AsRawFd for Kqueue {
        fn as_raw_fd(&self) -> RawFd {
            self.fd
        }
    }

    pub fn interpret_events(raw: &[RawKevent]) -> Vec<Event> {
        raw.iter()
            .map(|event| Event {
                fd: event.ident as RawFd,
                flags: event.fflags,
            })
            .collect()
    }

    pub const NOTE_DELETE: u32 = 0x0000_0001;
    pub const NOTE_WRITE: u32 = 0x0000_0002;
    pub const NOTE_EXTEND: u32 = 0x0000_0004;
    pub const NOTE_ATTRIB: u32 = 0x0000_0008;
    pub const NOTE_LINK: u32 = 0x0000_0010;
    pub const NOTE_RENAME: u32 = 0x0000_0020;
    pub const NOTE_REVOKE: u32 = 0x0000_0040;

    const EVFILT_VNODE: i16 = -4;
    const EV_ADD: u16 = 0x0001;
    const EV_ENABLE: u16 = 0x0004;
    const EV_CLEAR: u16 = 0x0020;

    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[repr(C)]
    struct RawKevent {
        ident: usize,
        filter: i16,
        flags: u16,
        fflags: u32,
        data: isize,
        udata: *mut std::ffi::c_void,
        ext: [u64; 4],
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    #[repr(C)]
    struct RawKevent {
        ident: usize,
        filter: i16,
        flags: u16,
        fflags: u32,
        data: isize,
        udata: isize,
    }

    impl RawKevent {
        fn zeroed() -> Self {
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
        fn udata_zero() -> *mut std::ffi::c_void {
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

        fn set_udata_null(&mut self) {
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

    mod ffi {
        use super::{RawKevent, Timespec};

        extern "C" {
            pub fn kqueue() -> i32;
            pub fn kevent(
                kq: i32,
                changelist: *const RawKevent,
                nchanges: i32,
                eventlist: *mut RawKevent,
                nevents: i32,
                timeout: *const Timespec,
            ) -> i32;
            pub fn close(fd: i32) -> i32;
        }
    }
}

pub mod process {
    #[cfg(target_os = "linux")]
    use crate::error::{Result, SysError};
    #[cfg(target_os = "linux")]
    use std::fs;

    /// Best-effort determination of the resident set size for the current process.
    pub fn resident_memory_bytes() -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            read_vmrss().ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    #[cfg(target_os = "linux")]
    fn read_vmrss() -> Result<u64> {
        let status = fs::read_to_string("/proc/self/status")?;
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                let mut parts = value.split_whitespace();
                let amount = parts
                    .next()
                    .ok_or_else(|| SysError::unsupported("VmRSS amount"))?;
                let unit = parts.next().unwrap_or("kB");
                let quantity: u64 = amount
                    .parse()
                    .map_err(|_| SysError::unsupported("VmRSS parse"))?;
                return match unit {
                    "B" => Ok(quantity),
                    "kB" => Ok(quantity.saturating_mul(1024)),
                    "MB" | "mB" => Ok(quantity.saturating_mul(1024 * 1024)),
                    "GB" | "gB" => Ok(quantity.saturating_mul(1024 * 1024 * 1024)),
                    _ => Err(SysError::unsupported("VmRSS unit")),
                };
            }
        }
        Err(SysError::unsupported("VmRSS missing"))
    }

    /// Return the effective user ID when available.
    pub fn effective_uid() -> Option<u32> {
        #[cfg(target_os = "linux")]
        {
            read_effective_uid().ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    #[cfg(target_os = "linux")]
    fn read_effective_uid() -> Result<u32> {
        let status = fs::read_to_string("/proc/self/status")?;
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("Uid:") {
                let mut parts = value.split_whitespace();
                let _real = parts
                    .next()
                    .ok_or_else(|| SysError::unsupported("Uid real"))?;
                let effective = parts
                    .next()
                    .ok_or_else(|| SysError::unsupported("Uid effective"))?;
                let parsed: u32 = effective
                    .parse()
                    .map_err(|_| SysError::unsupported("Uid parse"))?;
                return Ok(parsed);
            }
        }
        Err(SysError::unsupported("Uid missing"))
    }
}

pub mod random {
    use crate::error::Result;
    #[cfg(unix)]
    use std::io::Read;
    #[cfg(unix)]
    use std::sync::{Mutex, OnceLock};

    #[cfg(unix)]
    fn urandom() -> Result<&'static Mutex<std::fs::File>> {
        static URANDOM: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
        if let Some(reader) = URANDOM.get() {
            return Ok(reader);
        }
        let file = std::fs::File::open("/dev/urandom").map_err(crate::error::SysError::from)?;
        match URANDOM.set(Mutex::new(file)) {
            Ok(()) => Ok(URANDOM.get().expect("urandom file initialized")),
            Err(_) => Ok(URANDOM.get().expect("urandom file initialized")),
        }
    }

    #[cfg(unix)]
    pub fn fill_bytes(dest: &mut [u8]) -> Result<()> {
        if dest.is_empty() {
            return Ok(());
        }
        let reader = urandom()?;
        let mut guard = reader
            .lock()
            .map_err(|_| crate::error::SysError::unsupported("/dev/urandom mutex poisoned"))?;
        guard.read_exact(dest).map_err(crate::error::SysError::from)
    }

    #[cfg(not(unix))]
    pub fn fill_bytes(dest: &mut [u8]) -> Result<()> {
        if dest.is_empty() {
            return Ok(());
        }
        Err(crate::error::SysError::unsupported("os randomness"))
    }

    pub fn fill_u64() -> Result<u64> {
        let mut buf = [0u8; 8];
        fill_bytes(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }
}

pub mod tty {
    use crate::error::{Result, SysError};
    use std::io::{self, BufRead, IsTerminal, Write};

    #[cfg(unix)]
    pub fn dimensions() -> Option<(u16, u16)> {
        use crate::unix_ffi::{self, WinSize};
        use core::ffi::c_int;

        unsafe fn query(fd: c_int) -> Option<(u16, u16)> {
            let mut ws = WinSize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if unix_ffi::ioctl(fd, unix_ffi::TIOCGWINSZ, &mut ws) == 0 {
                let cols = ws.ws_col as u16;
                let rows = ws.ws_row as u16;
                if cols > 0 && rows > 0 {
                    return Some((cols, rows));
                }
            }
            None
        }

        unsafe { query(unix_ffi::STDOUT_FILENO).or_else(|| query(unix_ffi::STDIN_FILENO)) }
    }

    #[cfg(not(unix))]
    pub fn dimensions() -> Option<(u16, u16)> {
        None
    }

    /// Returns `true` when standard output is connected to a terminal.
    pub fn stdout_is_terminal() -> bool {
        io::stdout().is_terminal()
    }

    /// Returns `true` when standard error is connected to a terminal.
    pub fn stderr_is_terminal() -> bool {
        io::stderr().is_terminal()
    }

    /// Returns `true` when standard input is connected to a terminal.
    pub fn stdin_is_terminal() -> bool {
        io::stdin().is_terminal()
    }

    /// Prompt for a passphrase while disabling terminal echo when supported.
    pub fn read_passphrase(prompt: &str) -> Result<String> {
        read_passphrase_internal(prompt)
    }

    fn read_passphrase_internal(prompt: &str) -> Result<String> {
        let stdin = io::stdin();
        let stderr = io::stderr();
        let mut reader = stdin.lock();
        let mut writer = stderr.lock();
        read_passphrase_with(prompt, &mut reader, &mut writer, EchoGuard::activate)
    }

    fn read_passphrase_with<R, W, G, Guard>(
        prompt: &str,
        reader: &mut R,
        writer: &mut W,
        guard: G,
    ) -> Result<String>
    where
        R: BufRead,
        W: Write,
        G: FnOnce() -> Result<Guard>,
    {
        writer
            .write_all(prompt.as_bytes())
            .map_err(SysError::from)?;
        writer.flush().map_err(SysError::from)?;

        let guard = guard()?;

        let mut line = String::new();
        reader.read_line(&mut line).map_err(SysError::from)?;

        drop(guard);

        // Restore a consistent newline for the caller and separate the prompt.
        writer.write_all(b"\n").map_err(SysError::from)?;
        writer.flush().map_err(SysError::from)?;

        while matches!(line.chars().last(), Some('\n') | Some('\r')) {
            line.pop();
        }

        Ok(line)
    }

    struct EchoGuard {
        #[cfg(unix)]
        _unix: Option<UnixEchoGuard>,
        #[cfg(windows)]
        _windows: Option<WindowsEchoGuard>,
    }

    impl EchoGuard {
        fn activate() -> Result<Self> {
            Ok(Self {
                #[cfg(unix)]
                _unix: unix_guard()?,
                #[cfg(windows)]
                _windows: windows_guard()?,
            })
        }
    }

    #[cfg(unix)]
    struct UnixEchoGuard {
        fd: std::os::fd::RawFd,
        previous: crate::unix_ffi::Termios,
    }

    #[cfg(unix)]
    impl Drop for UnixEchoGuard {
        fn drop(&mut self) {
            unsafe {
                let _ =
                    crate::unix_ffi::tcsetattr(self.fd, crate::unix_ffi::TCSANOW, &self.previous);
            }
        }
    }

    #[cfg(unix)]
    fn unix_guard() -> Result<Option<UnixEchoGuard>> {
        use std::mem::MaybeUninit;
        use std::os::fd::AsRawFd;

        if !stdin_is_terminal() {
            return Ok(None);
        }

        let stdin = io::stdin();
        let fd = stdin.as_raw_fd();
        let mut current = MaybeUninit::<crate::unix_ffi::Termios>::uninit();
        unsafe {
            if crate::unix_ffi::tcgetattr(fd, current.as_mut_ptr()) != 0 {
                return Err(SysError::from(io::Error::last_os_error()));
            }
            let mut current = current.assume_init();
            let previous = current;
            current.c_lflag &= !crate::unix_ffi::ECHO;
            if crate::unix_ffi::tcsetattr(fd, crate::unix_ffi::TCSANOW, &current) != 0 {
                return Err(SysError::from(io::Error::last_os_error()));
            }
            Ok(Some(UnixEchoGuard { fd, previous }))
        }
    }

    #[cfg(windows)]
    struct WindowsEchoGuard {
        handle: std::os::windows::io::RawHandle,
        previous_mode: u32,
    }

    #[cfg(windows)]
    impl Drop for WindowsEchoGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = windows::SetConsoleMode(self.handle as isize, self.previous_mode);
            }
        }
    }

    #[cfg(windows)]
    fn windows_guard() -> Result<Option<WindowsEchoGuard>> {
        use std::os::windows::io::AsRawHandle;

        if !stdin_is_terminal() {
            return Ok(None);
        }

        let stdin = io::stdin();
        let handle = stdin.as_raw_handle();
        unsafe {
            let mut mode = 0u32;
            if windows::GetConsoleMode(handle as isize, &mut mode) == 0 {
                return Ok(None);
            }
            let new_mode = mode & !windows::ENABLE_ECHO_INPUT;
            if new_mode == mode {
                return Ok(Some(WindowsEchoGuard {
                    handle,
                    previous_mode: mode,
                }));
            }
            if windows::SetConsoleMode(handle as isize, new_mode) == 0 {
                return Err(SysError::from(io::Error::last_os_error()));
            }
            Ok(Some(WindowsEchoGuard {
                handle,
                previous_mode: mode,
            }))
        }
    }

    #[cfg(windows)]
    mod windows {
        pub const ENABLE_ECHO_INPUT: u32 = 0x0004;

        #[link(name = "kernel32")]
        extern "system" {
            pub fn GetConsoleMode(handle: isize, mode: *mut u32) -> i32;
            pub fn SetConsoleMode(handle: isize, mode: u32) -> i32;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::Cell;
        use std::io::Cursor;

        #[test]
        fn read_passphrase_trims_newlines() {
            let mut output = Vec::new();
            let mut input = Cursor::new(b"secret\r\n".as_slice());
            let invoked = Cell::new(false);
            let guard = || {
                invoked.set(true);
                Result::<()>::Ok(())
            };

            let result = read_passphrase_with("Prompt: ", &mut input, &mut output, guard)
                .expect("passphrase");

            assert_eq!(result, "secret");
            assert!(invoked.get());
            assert_eq!(output, b"Prompt: \n");
        }

        #[test]
        fn read_passphrase_allows_empty_input() {
            let mut output = Vec::new();
            let mut input = Cursor::new(b"\n".as_slice());
            let guard = || Result::<()>::Ok(());

            let result = read_passphrase_with("Prompt: ", &mut input, &mut output, guard)
                .expect("passphrase");

            assert_eq!(result, "");
            assert_eq!(output, b"Prompt: \n");
        }
    }
}

pub mod signals {
    use crate::error::Result;
    #[cfg(unix)]
    use crate::error::SysError;
    use std::vec::IntoIter;

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::unix_ffi;
        use core::ffi::{c_int, c_void};
        use std::collections::{HashSet, VecDeque};
        use std::fs::File;
        use std::io::{self, Read};
        use std::mem;
        use std::os::fd::{FromRawFd, RawFd};
        use std::sync::atomic::{AtomicI32, Ordering};
        use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
        use std::thread;

        pub const SIGHUP: i32 = unix_ffi::SIGHUP;

        static WRITE_FD: AtomicI32 = AtomicI32::new(-1);
        static DISPATCHER: OnceLock<Dispatcher> = OnceLock::new();

        #[derive(Default)]
        struct SignalQueue {
            buffer: Mutex<VecDeque<i32>>,
            ready: Condvar,
        }

        impl SignalQueue {
            fn push(&self, signal: i32) {
                let mut guard = self.buffer.lock().expect("signal queue lock");
                guard.push_back(signal);
                self.ready.notify_all();
            }

            fn drain(&self, filter: &HashSet<i32>) -> Vec<i32> {
                let mut guard = self.buffer.lock().expect("signal queue lock");
                Self::drain_filtered(&mut guard, filter)
            }

            fn wait_next(&self, filter: &HashSet<i32>) -> Option<i32> {
                let mut guard = self.buffer.lock().expect("signal queue lock");
                loop {
                    if let Some(signal) = Self::drain_one(&mut guard, filter) {
                        return Some(signal);
                    }
                    guard = self.ready.wait(guard).expect("signal queue wait");
                }
            }

            fn drain_one(queue: &mut VecDeque<i32>, filter: &HashSet<i32>) -> Option<i32> {
                let len = queue.len();
                for _ in 0..len {
                    if let Some(signal) = queue.pop_front() {
                        if filter.is_empty() || filter.contains(&signal) {
                            return Some(signal);
                        }
                        queue.push_back(signal);
                    }
                }
                None
            }

            fn drain_filtered(queue: &mut VecDeque<i32>, filter: &HashSet<i32>) -> Vec<i32> {
                let len = queue.len();
                let mut drained = Vec::new();
                for _ in 0..len {
                    if let Some(signal) = queue.pop_front() {
                        if filter.is_empty() || filter.contains(&signal) {
                            drained.push(signal);
                        } else {
                            queue.push_back(signal);
                        }
                    }
                }
                drained
            }
        }

        struct Dispatcher {
            read_fd: RawFd,
            queues: Mutex<Vec<Weak<SignalQueue>>>,
            registered: Mutex<HashSet<i32>>,
        }

        impl Dispatcher {
            fn global() -> Result<&'static Dispatcher> {
                if let Some(dispatcher) = DISPATCHER.get() {
                    return Ok(dispatcher);
                }

                let (read_fd, write_fd) = create_pipe()?;
                WRITE_FD.store(write_fd, Ordering::SeqCst);
                let dispatcher = Dispatcher {
                    read_fd,
                    queues: Mutex::new(Vec::new()),
                    registered: Mutex::new(HashSet::new()),
                };
                match DISPATCHER.set(dispatcher) {
                    Ok(()) => {
                        let dispatcher = DISPATCHER.get().expect("dispatcher initialized");
                        dispatcher.spawn_reader();
                        Ok(dispatcher)
                    }
                    Err(_) => Ok(DISPATCHER.get().expect("dispatcher initialized")),
                }
            }

            fn spawn_reader(&self) {
                let read_fd = self.read_fd;
                thread::spawn(move || {
                    let mut file = unsafe { File::from_raw_fd(read_fd) };
                    let mut buf = [0u8; 128];
                    loop {
                        match file.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let dispatcher = match DISPATCHER.get() {
                                    Some(dispatcher) => dispatcher,
                                    None => break,
                                };
                                for chunk in buf[..n].chunks(mem::size_of::<i32>()) {
                                    if chunk.len() != mem::size_of::<i32>() {
                                        continue;
                                    }
                                    let mut bytes = [0u8; mem::size_of::<i32>()];
                                    bytes.copy_from_slice(chunk);
                                    let signal = i32::from_ne_bytes(bytes);
                                    dispatcher.dispatch(signal);
                                }
                            }
                            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                                thread::yield_now();
                            }
                            Err(_) => break,
                        }
                    }
                });
            }

            fn dispatch(&self, signal: i32) {
                let queues: Vec<Arc<SignalQueue>> = {
                    let mut guard = self.queues.lock().expect("dispatcher queues lock");
                    let mut active = Vec::new();
                    guard.retain(|weak| {
                        if let Some(queue) = weak.upgrade() {
                            active.push(queue);
                            true
                        } else {
                            false
                        }
                    });
                    active
                };

                for queue in queues {
                    queue.push(signal);
                }
            }

            fn add_queue(&self, queue: &Arc<SignalQueue>) {
                let mut guard = self.queues.lock().expect("dispatcher queues lock");
                guard.push(Arc::downgrade(queue));
            }

            fn register_signal(&self, signal: i32) -> Result<()> {
                let mut guard = self.registered.lock().expect("registered lock");
                if guard.contains(&signal) {
                    return Ok(());
                }
                unsafe {
                    let sa = unix_ffi::SigAction {
                        sa_sigaction: unix_ffi::SigActionHandler {
                            sigaction: signal_handler,
                        },
                        sa_mask: unix_ffi::SigSet::empty(),
                        sa_flags: unix_ffi::SA_SIGINFO | unix_ffi::SA_RESTART,
                        sa_restorer: None,
                    };
                    if unix_ffi::sigaction(signal as c_int, &sa, std::ptr::null_mut()) != 0 {
                        return Err(io::Error::last_os_error().into());
                    }
                }
                guard.insert(signal);
                Ok(())
            }
        }

        fn create_pipe() -> Result<(RawFd, RawFd)> {
            let mut fds = [0; 2];
            let res = unsafe { unix_ffi::pipe(fds.as_mut_ptr()) };
            if res != 0 {
                return Err(io::Error::last_os_error().into());
            }
            set_nonblocking(fds[0])?;
            set_nonblocking(fds[1])?;
            set_cloexec(fds[0])?;
            set_cloexec(fds[1])?;
            Ok((fds[0], fds[1]))
        }

        fn set_nonblocking(fd: RawFd) -> Result<()> {
            unsafe {
                let flags = unix_ffi::fcntl(fd, unix_ffi::F_GETFL);
                if flags == -1 {
                    return Err(io::Error::last_os_error().into());
                }
                if unix_ffi::fcntl(fd, unix_ffi::F_SETFL, flags | unix_ffi::O_NONBLOCK) == -1 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            Ok(())
        }

        fn set_cloexec(fd: RawFd) -> Result<()> {
            unsafe {
                let flags = unix_ffi::fcntl(fd, unix_ffi::F_GETFD);
                if flags == -1 {
                    return Err(io::Error::last_os_error().into());
                }
                if unix_ffi::fcntl(fd, unix_ffi::F_SETFD, flags | unix_ffi::FD_CLOEXEC) == -1 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            Ok(())
        }

        extern "C" fn signal_handler(signal: c_int, _: *mut unix_ffi::SigInfo, _: *mut c_void) {
            let fd = WRITE_FD.load(Ordering::Relaxed);
            if fd < 0 {
                return;
            }
            let bytes = signal.to_ne_bytes();
            unsafe {
                let _ = unix_ffi::write(fd, bytes.as_ptr() as *const c_void, bytes.len());
            }
        }

        pub struct Signals {
            queue: Arc<SignalQueue>,
            filter: HashSet<i32>,
        }

        impl Signals {
            pub fn new<I>(signals: I) -> Result<Self>
            where
                I: IntoIterator<Item = i32>,
            {
                let requested: HashSet<i32> = signals.into_iter().collect();
                if requested.is_empty() {
                    return Err(SysError::unsupported(
                        "signals::new requires at least one signal",
                    ));
                }

                let dispatcher = Dispatcher::global()?;
                for &signal in &requested {
                    dispatcher.register_signal(signal)?;
                }

                let queue = Arc::new(SignalQueue::default());
                dispatcher.add_queue(&queue);
                Ok(Self {
                    queue,
                    filter: requested,
                })
            }

            pub fn pending(&mut self) -> IntoIter<i32> {
                self.queue.drain(&self.filter).into_iter()
            }

            pub fn wait(&mut self) -> Option<i32> {
                self.queue.wait_next(&self.filter)
            }

            pub fn forever(self) -> ForeverSignals {
                ForeverSignals { inner: self }
            }
        }

        pub struct ForeverSignals {
            inner: Signals,
        }

        impl Iterator for ForeverSignals {
            type Item = i32;

            fn next(&mut self) -> Option<Self::Item> {
                self.inner.wait()
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;
            use std::thread;
            use std::time::Duration;

            #[test]
            fn pending_drains_signals() {
                let mut signals = Signals::new([SIGHUP]).expect("signals");
                unsafe {
                    unix_ffi::raise(SIGHUP);
                }
                thread::sleep(Duration::from_millis(25));
                let mut drained = signals.pending();
                assert_eq!(drained.next(), Some(SIGHUP));
            }

            #[test]
            fn wait_yields_signals() {
                let mut signals = Signals::new([SIGHUP]).expect("signals");
                thread::spawn(|| {
                    thread::sleep(Duration::from_millis(25));
                    unsafe {
                        unix_ffi::raise(SIGHUP);
                    }
                });
                assert_eq!(signals.wait(), Some(SIGHUP));
            }
        }
    }

    #[cfg(unix)]
    pub use unix::{ForeverSignals, Signals, SIGHUP};

    #[cfg(not(unix))]
    mod fallback {
        use super::*;

        pub const SIGHUP: i32 = 1;

        pub struct Signals {
            _signals: Vec<i32>,
        }

        impl Signals {
            pub fn new<I>(signals: I) -> Result<Self>
            where
                I: IntoIterator<Item = i32>,
            {
                Ok(Self {
                    _signals: signals.into_iter().collect(),
                })
            }

            pub fn pending(&mut self) -> IntoIter<i32> {
                Vec::new().into_iter()
            }

            pub fn wait(&mut self) -> Option<i32> {
                None
            }

            pub fn forever(self) -> ForeverSignals {
                ForeverSignals
            }
        }

        pub struct ForeverSignals;

        impl Iterator for ForeverSignals {
            type Item = i32;

            fn next(&mut self) -> Option<Self::Item> {
                None
            }
        }
    }

    #[cfg(not(unix))]
    pub use fallback::{ForeverSignals, Signals, SIGHUP};
}

pub mod tempfile {
    use crate::error::{Result, SysError};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_path(base: &Path, prefix: &str, suffix: &str) -> io::Result<PathBuf> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|dur| dur.as_nanos())
            .unwrap_or(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("{prefix}{nanos:x}{count:x}{suffix}");
        Ok(base.join(name))
    }

    pub struct TempDir {
        path: PathBuf,
        persist: bool,
    }

    impl TempDir {
        pub fn new() -> Result<Self> {
            Builder::new().tempdir()
        }

        pub fn new_in<P: AsRef<Path>>(base: P) -> Result<Self> {
            Builder::new().tempdir_in(base)
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        pub fn into_path(mut self) -> PathBuf {
            self.persist = true;
            self.path.clone()
        }

        pub fn keep(self) -> PathBuf {
            self.into_path()
        }

        pub fn close(mut self) -> Result<()> {
            let path = self.path.clone();
            self.persist = true;
            fs::remove_dir_all(path)?;
            Ok(())
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            if !self.persist {
                let _ = fs::remove_dir_all(&self.path);
            }
        }
    }

    pub struct Builder {
        prefix: Option<String>,
        suffix: Option<String>,
    }

    impl Builder {
        pub fn new() -> Self {
            Self {
                prefix: None,
                suffix: None,
            }
        }

        pub fn prefix(&mut self, value: &str) -> &mut Self {
            self.prefix = Some(value.to_string());
            self
        }

        pub fn suffix(&mut self, value: &str) -> &mut Self {
            self.suffix = Some(value.to_string());
            self
        }

        pub fn tempdir(&self) -> Result<TempDir> {
            self.tempdir_in(std::env::temp_dir())
        }

        pub fn tempdir_in<P: AsRef<Path>>(&self, base: P) -> Result<TempDir> {
            let base = base.as_ref();
            fs::create_dir_all(base)?;
            let prefix = self.prefix.as_deref().unwrap_or("sys-");
            let suffix = self.suffix.as_deref().unwrap_or("");
            let mut attempts = 0;
            loop {
                let path = unique_path(base, prefix, suffix)?;
                match fs::create_dir(&path) {
                    Ok(()) => {
                        return Ok(TempDir {
                            path,
                            persist: false,
                        })
                    }
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                        attempts += 1;
                        if attempts > 32 {
                            return Err(SysError::from(err));
                        }
                    }
                    Err(err) => return Err(SysError::from(err)),
                }
            }
        }
    }

    pub fn tempdir() -> Result<TempDir> {
        Builder::new().tempdir()
    }

    pub fn tempdir_in<P: AsRef<Path>>(base: P) -> Result<TempDir> {
        Builder::new().tempdir_in(base)
    }

    pub struct NamedTempFile {
        file: File,
        path: PathBuf,
        keep: bool,
    }

    impl NamedTempFile {
        pub fn new() -> Result<Self> {
            Self::new_in(std::env::temp_dir())
        }

        pub fn new_in<P: AsRef<Path>>(base: P) -> Result<Self> {
            let base = base.as_ref();
            fs::create_dir_all(base)?;
            let path = unique_path(base, "sys-file-", "")?;
            let file = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)?;
            Ok(Self {
                file,
                path,
                keep: false,
            })
        }

        pub fn as_file(&self) -> &File {
            &self.file
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        pub fn persist(mut self, dest: &Path) -> std::result::Result<PathBuf, PersistError> {
            self.file
                .sync_all()
                .map_err(|error| PersistError { error })?;
            if let Some(parent) = dest.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    return Err(PersistError { error });
                }
            }
            match fs::rename(&self.path, dest) {
                Ok(()) => {
                    self.keep = true;
                    Ok(dest.to_path_buf())
                }
                Err(error) => Err(PersistError { error }),
            }
        }
    }

    impl Drop for NamedTempFile {
        fn drop(&mut self) {
            if !self.keep {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    impl Write for NamedTempFile {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.file.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    pub struct PersistError {
        pub error: io::Error,
    }

    impl From<io::Error> for PersistError {
        fn from(error: io::Error) -> Self {
            PersistError { error }
        }
    }
}

pub mod fs {
    use crate::error::{Result, SysError};
    use std::fs::File;
    #[cfg(unix)]
    use std::os::fd::AsRawFd;

    #[cfg(target_os = "windows")]
    pub mod windows;

    #[cfg(unix)]
    pub const O_NOFOLLOW: i32 = crate::unix_ffi::O_NOFOLLOW;
    #[cfg(not(unix))]
    pub const O_NOFOLLOW: i32 = 0;

    pub trait FileLockExt {
        fn lock_exclusive(&self) -> Result<()>;
    }

    impl FileLockExt for File {
        fn lock_exclusive(&self) -> Result<()> {
            #[cfg(unix)]
            {
                let fd = self.as_raw_fd();
                let result = unsafe { crate::unix_ffi::flock(fd, crate::unix_ffi::LOCK_EX) };
                if result == 0 {
                    Ok(())
                } else {
                    Err(SysError::from(std::io::Error::last_os_error()))
                }
            }
            #[cfg(not(unix))]
            {
                Err(SysError::unsupported("file locking"))
            }
        }
    }
}

pub mod archive {
    /// Minimal ZIP archive helpers supporting the subset of functionality used
    /// by in-house tooling. The implementation is intentionally small and keeps
    /// everything in memory, mirroring the previous `zip` crate behaviour while
    /// avoiding a third-party dependency.
    pub mod zip {
        use crypto_suite::hashing::crc32;
        use std::collections::HashSet;
        use std::convert::TryInto;
        use std::error::Error;
        use std::fmt;

        const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x0403_4b50;
        const CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0201_4b50;
        const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0605_4b50;

        /// Result alias for archive operations.
        pub type Result<T> = std::result::Result<T, ZipError>;

        /// Builder used to assemble an in-memory ZIP archive.
        #[derive(Default)]
        pub struct ZipBuilder {
            entries: Vec<Entry>,
            names: HashSet<String>,
        }

        #[derive(Clone)]
        struct Entry {
            name: String,
            data: Vec<u8>,
        }

        impl ZipBuilder {
            /// Create a new builder with no entries.
            pub fn new() -> Self {
                Self::default()
            }

            /// Add a file to the archive.
            pub fn add_file(&mut self, name: &str, data: &[u8]) -> Result<()> {
                if name.is_empty() {
                    return Err(ZipError::InvalidArchive("file name cannot be empty"));
                }
                if name.len() > u16::MAX as usize {
                    return Err(ZipError::FileNameTooLong(name.to_string()));
                }
                if self.entries.len() >= u16::MAX as usize {
                    return Err(ZipError::TooManyEntries);
                }
                if !self.names.insert(name.to_string()) {
                    return Err(ZipError::DuplicateEntry(name.to_string()));
                }
                self.entries.push(Entry {
                    name: name.to_string(),
                    data: data.to_vec(),
                });
                Ok(())
            }

            /// Finalize the archive, returning the serialized bytes.
            pub fn finish(self) -> Result<Vec<u8>> {
                let mut writer = Vec::new();
                let mut central_directory = Vec::new();
                let mut offset = 0u32;

                for entry in &self.entries {
                    let name_len: u16 = entry
                        .name
                        .len()
                        .try_into()
                        .map_err(|_| ZipError::FileNameTooLong(entry.name.clone()))?;
                    let size: u32 = entry
                        .data
                        .len()
                        .try_into()
                        .map_err(|_| ZipError::ArchiveTooLarge)?;
                    let crc =
                        crc32::checksum(&entry.data).expect("crc32 implementation is available");

                    write_u32(&mut writer, LOCAL_FILE_HEADER_SIGNATURE);
                    write_u16(&mut writer, 20); // version needed to extract
                    write_u16(&mut writer, 0); // flags
                    write_u16(&mut writer, 0); // stored (no compression)
                    write_u16(&mut writer, 0); // mod time
                    write_u16(&mut writer, 0); // mod date
                    write_u32(&mut writer, crc);
                    write_u32(&mut writer, size);
                    write_u32(&mut writer, size);
                    write_u16(&mut writer, name_len);
                    write_u16(&mut writer, 0); // extra length
                    writer.extend_from_slice(entry.name.as_bytes());
                    writer.extend_from_slice(&entry.data);

                    let local_offset = offset;
                    offset = writer
                        .len()
                        .try_into()
                        .map_err(|_| ZipError::ArchiveTooLarge)?;

                    write_u32(&mut central_directory, CENTRAL_DIRECTORY_SIGNATURE);
                    write_u16(&mut central_directory, 20); // version made by
                    write_u16(&mut central_directory, 20); // version needed
                    write_u16(&mut central_directory, 0); // flags
                    write_u16(&mut central_directory, 0); // method
                    write_u16(&mut central_directory, 0); // mod time
                    write_u16(&mut central_directory, 0); // mod date
                    write_u32(&mut central_directory, crc);
                    write_u32(&mut central_directory, size);
                    write_u32(&mut central_directory, size);
                    write_u16(&mut central_directory, name_len);
                    write_u16(&mut central_directory, 0); // extra length
                    write_u16(&mut central_directory, 0); // file comment length
                    write_u16(&mut central_directory, 0); // disk number start
                    write_u16(&mut central_directory, 0); // internal attrs
                    write_u32(&mut central_directory, 0); // external attrs
                    write_u32(&mut central_directory, local_offset);
                    central_directory.extend_from_slice(entry.name.as_bytes());
                }

                let central_offset: u32 = writer
                    .len()
                    .try_into()
                    .map_err(|_| ZipError::ArchiveTooLarge)?;
                writer.extend_from_slice(&central_directory);
                let end_len: u32 = writer
                    .len()
                    .try_into()
                    .map_err(|_| ZipError::ArchiveTooLarge)?;
                let central_size = end_len
                    .checked_sub(central_offset)
                    .ok_or(ZipError::ArchiveTooLarge)?;

                write_u32(&mut writer, END_OF_CENTRAL_DIRECTORY_SIGNATURE);
                write_u16(&mut writer, 0); // disk number
                write_u16(&mut writer, 0); // start disk
                let entry_count: u16 = self
                    .entries
                    .len()
                    .try_into()
                    .map_err(|_| ZipError::TooManyEntries)?;
                write_u16(&mut writer, entry_count);
                write_u16(&mut writer, entry_count);
                write_u32(&mut writer, central_size);
                write_u32(&mut writer, central_offset);
                write_u16(&mut writer, 0); // comment length

                Ok(writer)
            }
        }

        /// Reader for archives produced by [`ZipBuilder`].
        pub struct ZipReader<'a> {
            entries: Vec<ZipEntry<'a>>,
        }

        /// Entry representation returned by the reader.
        pub struct ZipEntry<'a> {
            name: String,
            data: &'a [u8],
        }

        impl<'a> ZipEntry<'a> {
            /// Name of the file inside the archive.
            pub fn name(&self) -> &str {
                &self.name
            }

            /// File contents.
            pub fn data(&self) -> &'a [u8] {
                self.data
            }
        }

        impl<'a> ZipReader<'a> {
            /// Parse a ZIP archive from bytes.
            pub fn from_bytes(data: &'a [u8]) -> Result<Self> {
                let eocd_pos = find_eocd(data).ok_or(ZipError::InvalidArchive("missing EOCD"))?;
                if data.len() < eocd_pos + 22 {
                    return Err(ZipError::InvalidArchive("truncated EOCD"));
                }
                let total_entries = read_u16(&data[eocd_pos + 10..]) as usize;
                let central_size = read_u32(&data[eocd_pos + 12..]) as usize;
                let central_offset = read_u32(&data[eocd_pos + 16..]) as usize;
                let comment_len = read_u16(&data[eocd_pos + 20..]) as usize;
                if comment_len != 0 {
                    return Err(ZipError::InvalidArchive("archive comments unsupported"));
                }
                if central_offset + central_size > data.len() {
                    return Err(ZipError::InvalidArchive("central directory out of range"));
                }

                let mut entries = Vec::with_capacity(total_entries);
                let mut offset = central_offset;
                for _ in 0..total_entries {
                    if offset + 46 > data.len() {
                        return Err(ZipError::InvalidArchive("central directory truncated"));
                    }
                    if read_u32(&data[offset..]) != CENTRAL_DIRECTORY_SIGNATURE {
                        return Err(ZipError::InvalidArchive("bad central directory signature"));
                    }
                    let compression = read_u16(&data[offset + 10..]);
                    if compression != 0 {
                        return Err(ZipError::InvalidArchive("unsupported compression"));
                    }
                    let crc = read_u32(&data[offset + 16..]);
                    let compressed_size = read_u32(&data[offset + 20..]) as usize;
                    let uncompressed_size = read_u32(&data[offset + 24..]) as usize;
                    if compressed_size != uncompressed_size {
                        return Err(ZipError::InvalidArchive("compressed size mismatch"));
                    }
                    let name_len = read_u16(&data[offset + 28..]) as usize;
                    let extra_len = read_u16(&data[offset + 30..]) as usize;
                    let comment_len = read_u16(&data[offset + 32..]) as usize;
                    let local_offset = read_u32(&data[offset + 42..]) as usize;
                    let header_size = 46 + name_len + extra_len + comment_len;
                    if offset + header_size > data.len() {
                        return Err(ZipError::InvalidArchive("central directory overflow"));
                    }
                    let name_bytes = &data[offset + 46..offset + 46 + name_len];
                    let name = std::str::from_utf8(name_bytes)
                        .map_err(|_| ZipError::InvalidArchive("file name not utf-8"))?
                        .to_string();
                    offset += header_size;

                    if local_offset + 30 > data.len() {
                        return Err(ZipError::InvalidArchive("local header truncated"));
                    }
                    if read_u32(&data[local_offset..]) != LOCAL_FILE_HEADER_SIGNATURE {
                        return Err(ZipError::InvalidArchive("bad local header signature"));
                    }
                    let local_name_len = read_u16(&data[local_offset + 26..]) as usize;
                    let local_extra_len = read_u16(&data[local_offset + 28..]) as usize;
                    let data_start = local_offset + 30 + local_name_len + local_extra_len;
                    let data_end = data_start + uncompressed_size;
                    if data_end > data.len() {
                        return Err(ZipError::InvalidArchive("file data out of range"));
                    }
                    let file_data = &data[data_start..data_end];
                    if crc32::checksum(file_data).expect("crc32 implementation is available") != crc
                    {
                        return Err(ZipError::InvalidArchive("crc mismatch"));
                    }
                    entries.push(ZipEntry {
                        name,
                        data: file_data,
                    });
                }

                Ok(Self { entries })
            }

            /// Number of files within the archive.
            pub fn len(&self) -> usize {
                self.entries.len()
            }

            /// Returns true when the archive contains no entries.
            pub fn is_empty(&self) -> bool {
                self.entries.is_empty()
            }

            /// Iterate over entries in insertion order.
            pub fn entries(&'a self) -> impl Iterator<Item = &'a ZipEntry<'a>> {
                self.entries.iter()
            }

            /// Fetch a file by name.
            pub fn file(&self, name: &str) -> Option<&'a [u8]> {
                self.entries
                    .iter()
                    .find(|entry| entry.name == name)
                    .map(|entry| entry.data)
            }
        }

        /// Errors that can arise when working with ZIP archives.
        #[derive(Debug, Clone)]
        pub enum ZipError {
            InvalidArchive(&'static str),
            FileNameTooLong(String),
            DuplicateEntry(String),
            TooManyEntries,
            ArchiveTooLarge,
        }

        impl fmt::Display for ZipError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    ZipError::InvalidArchive(reason) => write!(f, "invalid zip archive: {reason}"),
                    ZipError::FileNameTooLong(name) => {
                        write!(f, "zip entry name too long: {name}")
                    }
                    ZipError::DuplicateEntry(name) => {
                        write!(f, "duplicate zip entry: {name}")
                    }
                    ZipError::TooManyEntries => {
                        write!(f, "zip archive has too many entries")
                    }
                    ZipError::ArchiveTooLarge => write!(f, "zip archive exceeds 4GiB limit"),
                }
            }
        }

        impl Error for ZipError {}

        fn write_u16(buf: &mut Vec<u8>, value: u16) {
            buf.extend_from_slice(&value.to_le_bytes());
        }

        fn write_u32(buf: &mut Vec<u8>, value: u32) {
            buf.extend_from_slice(&value.to_le_bytes());
        }

        fn read_u16(buf: &[u8]) -> u16 {
            u16::from_le_bytes([buf[0], buf[1]])
        }

        fn read_u32(buf: &[u8]) -> u32 {
            u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
        }

        fn find_eocd(data: &[u8]) -> Option<usize> {
            if data.len() < 22 {
                return None;
            }
            for index in (0..=data.len() - 4).rev() {
                if read_u32(&data[index..]) == END_OF_CENTRAL_DIRECTORY_SIGNATURE {
                    return Some(index);
                }
            }
            None
        }

        #[cfg(test)]
        mod tests {
            use super::{ZipBuilder, ZipReader};

            #[test]
            fn round_trip_archive() {
                let mut builder = ZipBuilder::new();
                builder.add_file("example.txt", b"hello").unwrap();
                builder.add_file("other.json", b"{}\n").unwrap();
                let bytes = builder.finish().unwrap();
                let archive = ZipReader::from_bytes(&bytes).unwrap();
                assert_eq!(archive.len(), 2);
                assert_eq!(archive.file("example.txt").unwrap(), b"hello");
                assert_eq!(archive.file("other.json").unwrap(), b"{}\n");
            }

            #[test]
            fn detects_bad_crc() {
                let mut builder = ZipBuilder::new();
                builder.add_file("example.txt", b"hello").unwrap();
                let mut bytes = builder.finish().unwrap();
                // Flip a payload byte to force a CRC mismatch.
                let len = bytes.len();
                bytes[len - 2] ^= 0xFF;
                assert!(ZipReader::from_bytes(&bytes).is_err());
            }
        }
    }
}

pub use error::{Result, SysError};

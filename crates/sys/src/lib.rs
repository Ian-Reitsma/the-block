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

pub mod process {
    #[cfg(unix)]
    use crate::error::{Result, SysError};
    #[cfg(unix)]
    use std::fs;

    /// Best-effort determination of the resident set size for the current process.
    pub fn resident_memory_bytes() -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            read_statm().ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    #[cfg(target_os = "linux")]
    fn read_statm() -> Result<u64> {
        let data = fs::read_to_string("/proc/self/statm")?;
        let mut fields = data.split_whitespace();
        let _size = fields.next();
        let resident = fields
            .next()
            .ok_or_else(|| SysError::unsupported("/proc/self/statm resident"))?;
        let resident: u64 = resident
            .parse()
            .map_err(|_| SysError::unsupported("/proc/self/statm parse"))?;
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size <= 0 {
            return Err(SysError::unsupported("sysconf(_SC_PAGESIZE)"));
        }
        Ok(resident * page_size as u64)
    }

    /// Return the effective user ID when available.
    pub fn effective_uid() -> Option<u32> {
        #[cfg(unix)]
        {
            Some(unsafe { libc::geteuid() })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}

pub mod random {
    use crate::error::{Result, SysError};
    use std::io;
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    use std::io::Read;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn fill_bytes(dest: &mut [u8]) -> Result<()> {
        use libc::{c_void, getrandom};

        let mut offset = 0;
        while offset < dest.len() {
            let remaining = dest.len() - offset;
            let ptr = dest[offset..].as_mut_ptr() as *mut c_void;
            let filled = unsafe { getrandom(ptr, remaining, 0) };
            if filled < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(SysError::from(err));
            }
            offset += filled as usize;
        }
        Ok(())
    }

    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    pub fn fill_bytes(dest: &mut [u8]) -> Result<()> {
        use std::fs::File;
        use std::sync::{Mutex, OnceLock};

        static URANDOM: OnceLock<Mutex<File>> = OnceLock::new();

        let reader = URANDOM.get_or_try_init(|| {
            File::open("/dev/urandom")
                .map(Mutex::new)
                .map_err(SysError::from)
        })?;

        let mut guard = reader
            .lock()
            .map_err(|_| SysError::unsupported("/dev/urandom mutex poisoned"))?;
        guard.read_exact(dest)?;
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn fill_bytes(dest: &mut [u8]) -> Result<()> {
        if dest.is_empty() {
            return Ok(());
        }
        Err(SysError::unsupported("os randomness"))
    }

    pub fn fill_u64() -> Result<u64> {
        let mut buf = [0u8; 8];
        fill_bytes(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }
}

pub mod tty {
    #[cfg(unix)]
    pub fn dimensions() -> Option<(u16, u16)> {
        use libc::{ioctl, winsize, STDIN_FILENO, STDOUT_FILENO, TIOCGWINSZ};

        unsafe fn query(fd: libc::c_int) -> Option<(u16, u16)> {
            let mut ws = winsize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if ioctl(fd, TIOCGWINSZ, &mut ws) == 0 {
                if ws.ws_col > 0 && ws.ws_row > 0 {
                    return Some((ws.ws_col as u16, ws.ws_row as u16));
                }
            }
            None
        }

        unsafe { query(STDOUT_FILENO).or_else(|| query(STDIN_FILENO)) }
    }

    #[cfg(not(unix))]
    pub fn dimensions() -> Option<(u16, u16)> {
        None
    }
}

pub mod signals {
    use crate::error::{Result, SysError};
    use std::vec::IntoIter;

    #[cfg(unix)]
    mod unix {
        use super::*;
        use libc::{self, c_int};
        use std::collections::{HashSet, VecDeque};
        use std::fs::File;
        use std::io::{self, Read};
        use std::mem;
        use std::os::fd::{FromRawFd, RawFd};
        use std::sync::atomic::{AtomicI32, Ordering};
        use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
        use std::thread;

        pub const SIGHUP: i32 = libc::SIGHUP;

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
                    let mut sa: libc::sigaction = mem::zeroed();
                    sa.sa_sigaction = mem::transmute::<
                        unsafe extern "C" fn(c_int, *mut libc::siginfo_t, *mut libc::c_void),
                        usize,
                    >(signal_handler);
                    sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
                    libc::sigemptyset(&mut sa.sa_mask);
                    if libc::sigaction(signal as c_int, &sa, std::ptr::null_mut()) != 0 {
                        return Err(io::Error::last_os_error().into());
                    }
                }
                guard.insert(signal);
                Ok(())
            }
        }

        fn create_pipe() -> Result<(RawFd, RawFd)> {
            let mut fds = [0; 2];
            let res = unsafe { libc::pipe(fds.as_mut_ptr()) };
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
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags == -1 {
                    return Err(io::Error::last_os_error().into());
                }
                if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            Ok(())
        }

        fn set_cloexec(fd: RawFd) -> Result<()> {
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags == -1 {
                    return Err(io::Error::last_os_error().into());
                }
                if libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) == -1 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            Ok(())
        }

        unsafe extern "C" fn signal_handler(
            signal: c_int,
            _: *mut libc::siginfo_t,
            _: *mut libc::c_void,
        ) {
            let fd = WRITE_FD.load(Ordering::Relaxed);
            if fd < 0 {
                return;
            }
            let bytes = signal.to_ne_bytes();
            let _ = libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len());
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
                    libc::raise(SIGHUP);
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
                        libc::raise(SIGHUP);
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
    use std::os::fd::AsRawFd;

    #[cfg(unix)]
    pub const O_NOFOLLOW: i32 = libc::O_NOFOLLOW;
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
                let result = unsafe { libc::flock(fd, libc::LOCK_EX) };
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

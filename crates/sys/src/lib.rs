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

pub mod signals {
    use crate::error::Result;
    use std::vec::IntoIter;

    pub const SIGHUP: i32 = libc::SIGHUP;

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

pub use error::{Result, SysError};

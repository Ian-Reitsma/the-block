use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_name(prefix: &str, suffix: &str) -> String {
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("tb-{prefix}-{pid}-{count}{suffix}")
}

pub fn tempdir() -> io::Result<TempDir> {
    TempDir::new()
}

pub struct TempDir {
    path: PathBuf,
    keep: bool,
}

impl TempDir {
    pub fn new() -> io::Result<Self> {
        Self::new_in(std::env::temp_dir())
    }

    pub fn new_in<P: AsRef<Path>>(base: P) -> io::Result<Self> {
        let mut attempts = 0;
        loop {
            let candidate = base.as_ref().join(unique_name("dir", ""));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    return Ok(Self {
                        path: candidate,
                        keep: false,
                    });
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists && attempts < 16 => {
                    attempts += 1;
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn into_path(mut self) -> PathBuf {
        self.keep = true;
        self.path.clone()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub struct NamedTempFile {
    path: PathBuf,
    file: Option<File>,
    keep: bool,
}

impl NamedTempFile {
    pub fn new_in(dir: &Path) -> io::Result<Self> {
        let mut attempts = 0;
        loop {
            let candidate = dir.join(unique_name("file", ".tmp"));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => {
                    return Ok(Self {
                        path: candidate,
                        file: Some(file),
                        keep: false,
                    });
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists && attempts < 16 => {
                    attempts += 1;
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn as_file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary file closed")
    }

    pub fn persist(mut self, new_path: &Path) -> Result<PathBuf, PersistError> {
        let target = new_path.to_path_buf();
        if let Some(file) = self.file.take() {
            if let Err(error) = file.sync_all() {
                self.file = Some(file);
                self.keep = false;
                return Err(PersistError {
                    error,
                    path: target,
                });
            }
        }
        match fs::rename(&self.path, &target) {
            Ok(()) => {
                self.keep = true;
                Ok(target)
            }
            Err(error) => {
                self.keep = false;
                Err(PersistError {
                    error,
                    path: target,
                })
            }
        }
    }
}

impl Write for NamedTempFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file
            .as_mut()
            .expect("temporary file closed")
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().expect("temporary file closed").flush()
    }
}

impl Drop for NamedTempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug)]
pub struct PersistError {
    pub error: io::Error,
    pub path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn tempdir_drops_cleanly() {
        let path = {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().to_path_buf();
            assert!(path.exists(), "directory should exist while TempDir lives");
            path
        };
        assert!(!path.exists(), "directory should be removed on drop");
    }

    #[test]
    fn tempdir_into_path_preserves_directory() {
        let path = {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.into_path();
            assert!(path.exists(), "directory should exist after into_path");
            path
        };
        fs::remove_dir_all(&path).expect("cleanup");
    }

    #[test]
    fn named_tempfile_persist_writes_contents() {
        let dir = TempDir::new().expect("tempdir");
        let mut file = NamedTempFile::new_in(dir.path()).expect("tempfile");
        file.as_file_mut().write_all(b"hello world").expect("write");
        file.flush().expect("flush");
        let dest = dir.path().join("persisted.txt");
        let persisted = file.persist(&dest).expect("persist");
        assert_eq!(persisted, dest);
        let read_back = std::fs::read_to_string(&persisted).expect("read persisted");
        assert_eq!(read_back, "hello world");
    }

    #[test]
    fn named_tempfile_persist_failure_cleans_up() {
        let dir = TempDir::new().expect("tempdir");
        let mut file = NamedTempFile::new_in(dir.path()).expect("tempfile");
        file.as_file_mut().write_all(b"payload").expect("write");
        let original_path = file.path().to_path_buf();
        let dest = dir.path().join("missing").join("persisted.txt");
        assert!(
            original_path.exists(),
            "temporary file should exist before persist"
        );
        let err = file.persist(&dest).expect_err("persist should fail");
        assert_eq!(err.path, dest);
        assert!(
            !original_path.exists(),
            "temporary file should be cleaned after failed persist"
        );
    }
}

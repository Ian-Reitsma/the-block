use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const RENAME_RETRIES: u32 = 5;

/// Write `bytes` to `path` atomically.
///
/// The data is written to `<path>.tmp`, fsynced, then atomically renamed over
/// `path`. Finally, the parent directory is fsynced on platforms that support
/// it.
pub fn write_atomic<P: AsRef<Path>>(path: P, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let tmp_path = tmp_path(path);

    // Ensure only one writer creates the temp file at a time.
    let mut create_attempts = 0;
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                break;
            }
            Err(e)
                if e.kind() == io::ErrorKind::AlreadyExists && create_attempts < RENAME_RETRIES =>
            {
                create_attempts += 1;
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    let mut attempt = 0;
    loop {
        match fs::rename(&tmp_path, path) {
            Ok(_) => break,
            Err(e)
                if cfg!(windows)
                    && e.kind() == io::ErrorKind::PermissionDenied
                    && attempt < RENAME_RETRIES =>
            {
                attempt += 1;
                thread::sleep(Duration::from_millis(50 * attempt as u64));
                continue;
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp_path);
                return Err(e);
            }
        }
    }

    fsync_parent(path)?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".tmp");
    PathBuf::from(os)
}

#[cfg(target_family = "unix")]
fn fsync_parent(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let dir = File::open(parent)?;
    dir.sync_all()
}

#[cfg(not(target_family = "unix"))]
fn fsync_parent(_path: &Path) -> io::Result<()> {
    // Platform lacks directory fsync (e.g., Windows). No-op.
    Ok(())
}

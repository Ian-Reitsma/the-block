use credits::CreditError;
use std::io;

#[cfg(target_os = "windows")]
const NO_SPACE_CODE: i32 = 112; // ERROR_DISK_FULL
#[cfg(unix)]
const NO_SPACE_CODE: i32 = 28; // ENOSPC
#[cfg(not(any(target_os = "windows", unix)))]
const NO_SPACE_CODE: i32 = 28; // default to ENOSPC

/// Convert a `CreditError` into an OS-specific `io::Error`.
///
/// Insufficient credits map to the platform's "no space" error so that
/// SMB/WebDAV clients surface a friendly "disk full" message.
pub fn credit_err_to_io(err: CreditError) -> io::Error {
    match err {
        CreditError::Insufficient => io::Error::from_raw_os_error(NO_SPACE_CODE),
        other => io::Error::new(io::ErrorKind::Other, other.to_string()),
    }
}

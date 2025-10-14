#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::io;
use std::mem::{self, MaybeUninit};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED,
    FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SECURITY,
    FILE_NOTIFY_CHANGE_SIZE, FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, CreateIoCompletionPort, GetQueuedCompletionStatusEx, PostQueuedCompletionStatus,
    OVERLAPPED, OVERLAPPED_ENTRY,
};

const WATCH_MASK: u32 = FILE_NOTIFY_CHANGE_FILE_NAME
    | FILE_NOTIFY_CHANGE_DIR_NAME
    | FILE_NOTIFY_CHANGE_ATTRIBUTES
    | FILE_NOTIFY_CHANGE_LAST_WRITE
    | FILE_NOTIFY_CHANGE_CREATION
    | FILE_NOTIFY_CHANGE_SIZE
    | FILE_NOTIFY_CHANGE_SECURITY;

const DIRECTORY_COMPLETION_KEY: usize = 1;
const SHUTDOWN_COMPLETION_KEY: usize = usize::MAX;
const MAX_COMPLETIONS: usize = 16;

#[derive(Clone)]
struct CompletionPort {
    handle: Arc<CompletionPortInner>,
}

struct CompletionPortInner {
    handle: HANDLE,
}

impl Drop for CompletionPortInner {
    fn drop(&mut self) {
        unsafe {
            if self.handle != 0 {
                CloseHandle(self.handle);
            }
        }
    }
}

impl CompletionPort {
    fn new() -> io::Result<Self> {
        let handle = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, 0, 0, 0) };
        if handle == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self {
                handle: Arc::new(CompletionPortInner { handle }),
            })
        }
    }

    fn raw(&self) -> HANDLE {
        self.handle.handle
    }

    fn associate(&self, file: HANDLE, key: usize) -> io::Result<()> {
        let result = unsafe { CreateIoCompletionPort(file, self.raw(), key, 0) };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn post(&self, key: usize) -> io::Result<()> {
        let result = unsafe { PostQueuedCompletionStatus(self.raw(), 0, key, ptr::null_mut()) };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[repr(C)]
struct CompletionContext {
    overlapped: OVERLAPPED,
    buffer: Vec<u8>,
}

impl CompletionContext {
    fn new(size: usize) -> Self {
        Self {
            overlapped: unsafe { mem::zeroed() },
            buffer: vec![0u8; size],
        }
    }

    fn reset(&mut self) {
        self.overlapped = unsafe { mem::zeroed() };
    }
}

unsafe impl Send for CompletionContext {}

#[derive(Clone)]
pub struct DirectoryChangeSignal {
    port: CompletionPort,
}

impl DirectoryChangeSignal {
    pub fn wake(&self) -> io::Result<()> {
        self.port.post(SHUTDOWN_COMPLETION_KEY)
    }
}

#[derive(Clone, Debug)]
pub enum DirectoryAction {
    Added,
    Removed,
    Modified,
    RenamedOld,
    RenamedNew,
    Other,
}

#[derive(Clone, Debug)]
pub struct DirectoryChange {
    pub path: PathBuf,
    pub action: DirectoryAction,
}

pub struct DirectoryChangeDriver {
    port: CompletionPort,
    directory: OwnedHandle,
    root: PathBuf,
    recursive: bool,
    context: Box<CompletionContext>,
    entries: [OVERLAPPED_ENTRY; MAX_COMPLETIONS],
}

unsafe impl Send for DirectoryChangeDriver {}

impl DirectoryChangeDriver {
    pub fn new(
        directory: OwnedHandle,
        root: PathBuf,
        recursive: bool,
        buffer_size: usize,
    ) -> io::Result<(Self, DirectoryChangeSignal)> {
        let port = CompletionPort::new()?;
        let mut driver = Self {
            port: port.clone(),
            directory,
            root,
            recursive,
            context: Box::new(CompletionContext::new(buffer_size)),
            entries: [Self::zero_entry(); MAX_COMPLETIONS],
        };
        driver.port.associate(
            driver.directory.as_raw_handle() as HANDLE,
            DIRECTORY_COMPLETION_KEY,
        )?;
        driver.issue_read()?;
        let signal = DirectoryChangeSignal { port };
        Ok((driver, signal))
    }

    pub fn poll(&mut self, timeout: Duration) -> io::Result<Option<Vec<DirectoryChange>>> {
        self.entries = [Self::zero_entry(); MAX_COMPLETIONS];
        let mut removed = 0u32;
        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let result = unsafe {
            GetQueuedCompletionStatusEx(
                self.port.raw(),
                self.entries.as_mut_ptr(),
                self.entries.len() as u32,
                &mut removed,
                timeout_ms,
                0,
            )
        };

        if result == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(WAIT_TIMEOUT as i32) {
                return Ok(None);
            }
            return Err(err);
        }

        for entry in self.entries.iter().take(removed as usize) {
            if entry.lpCompletionKey == SHUTDOWN_COMPLETION_KEY {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "watcher shutdown",
                ));
            }

            if entry.lpOverlapped.is_null() {
                continue;
            }

            if entry.Internal != 0 {
                let os_err = entry.Internal as i32;
                if os_err == ERROR_OPERATION_ABORTED as i32 {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "watcher cancelled",
                    ));
                }
                return Err(io::Error::from_raw_os_error(os_err));
            }

            let context_ptr = entry.lpOverlapped as *mut CompletionContext;
            let context = unsafe { &mut *context_ptr };
            let bytes = entry.dwNumberOfBytesTransferred as usize;
            let changes = self.parse_changes(context, bytes);
            self.issue_read()?;
            return Ok(Some(changes));
        }

        Ok(None)
    }

    pub fn cancel(&mut self) -> io::Result<()> {
        unsafe {
            CancelIoEx(
                self.directory.as_raw_handle() as HANDLE,
                &mut self.context.overlapped as *mut OVERLAPPED,
            );
        }
        self.port.post(SHUTDOWN_COMPLETION_KEY)
    }

    fn issue_read(&mut self) -> io::Result<()> {
        self.context.reset();
        let result = unsafe {
            ReadDirectoryChangesW(
                self.directory.as_raw_handle() as HANDLE,
                self.context.buffer.as_mut_ptr().cast(),
                self.context.buffer.len() as u32,
                self.recursive as i32,
                WATCH_MASK,
                ptr::null_mut(),
                &mut self.context.overlapped,
                None,
            )
        };
        if result == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_IO_PENDING as i32) {
                Ok(())
            } else {
                Err(err)
            }
        } else {
            Ok(())
        }
    }

    fn zero_entry() -> OVERLAPPED_ENTRY {
        unsafe { MaybeUninit::<OVERLAPPED_ENTRY>::zeroed().assume_init() }
    }

    fn parse_changes(&self, context: &CompletionContext, bytes: usize) -> Vec<DirectoryChange> {
        let mut changes = Vec::new();
        let mut offset = 0usize;
        while offset + mem::size_of::<FILE_NOTIFY_INFORMATION>() <= bytes {
            let record = unsafe {
                &*(context.buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION)
            };
            let name_len = (record.FileNameLength as usize) / 2;
            let name_slice =
                unsafe { std::slice::from_raw_parts(record.FileName.as_ptr(), name_len) };
            let name = OsString::from_wide(name_slice);
            let path = self.root.join(PathBuf::from(name));
            let action = match record.Action as u32 {
                FILE_ACTION_ADDED => DirectoryAction::Added,
                FILE_ACTION_REMOVED => DirectoryAction::Removed,
                FILE_ACTION_MODIFIED => DirectoryAction::Modified,
                FILE_ACTION_RENAMED_OLD_NAME => DirectoryAction::RenamedOld,
                FILE_ACTION_RENAMED_NEW_NAME => DirectoryAction::RenamedNew,
                _ => DirectoryAction::Other,
            };
            changes.push(DirectoryChange { path, action });
            if record.NextEntryOffset == 0 {
                break;
            }
            offset += record.NextEntryOffset as usize;
        }
        changes
    }
}

pub fn open_directory_handle(path: &Path) -> io::Result<OwnedHandle> {
    let wide: Vec<u16> = to_wide(path);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
    }
}

fn to_wide(path: &Path) -> Vec<u16> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide
}

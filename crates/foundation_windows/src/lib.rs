#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
pub mod foundation {
    pub type HANDLE = isize;
    pub const INVALID_HANDLE_VALUE: HANDLE = -1isize;
    pub const ERROR_IO_PENDING: u32 = 997;
    pub const ERROR_OPERATION_ABORTED: u32 = 995;
    pub const WAIT_TIMEOUT: u32 = 0x0000_0102;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CloseHandle(handle: HANDLE) -> i32;
        pub fn GetLastError() -> u32;
    }
}

#[cfg(not(windows))]
pub mod foundation {
    pub type HANDLE = isize;
    pub const INVALID_HANDLE_VALUE: HANDLE = -1isize;
    pub const ERROR_IO_PENDING: u32 = 997;
    pub const ERROR_OPERATION_ABORTED: u32 = 995;
    pub const WAIT_TIMEOUT: u32 = 0x0000_0102;

    #[inline(always)]
    pub unsafe fn CloseHandle(_handle: HANDLE) -> i32 {
        panic!("CloseHandle is only available on Windows targets");
    }

    #[inline(always)]
    pub fn GetLastError() -> u32 {
        panic!("GetLastError is only available on Windows targets");
    }
}

#[cfg(windows)]
pub mod io {
    use super::foundation::HANDLE;
    use core::ffi::c_void;

    #[repr(C)]
    pub struct OVERLAPPED {
        pub Internal: usize,
        pub InternalHigh: usize,
        pub Anonymous: OVERLAPPED_Union,
        pub hEvent: HANDLE,
    }

    #[repr(C)]
    pub union OVERLAPPED_Union {
        pub Pointer: *mut c_void,
        pub Offset: OVERLAPPED_Offset,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct OVERLAPPED_Offset {
        pub Offset: u32,
        pub OffsetHigh: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct OVERLAPPED_ENTRY {
        pub lpCompletionKey: usize,
        pub lpOverlapped: *mut OVERLAPPED,
        pub Internal: usize,
        pub dwNumberOfBytesTransferred: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CancelIoEx(file_handle: HANDLE, lp_overlapped: *mut OVERLAPPED) -> i32;
        pub fn CreateIoCompletionPort(
            file_handle: HANDLE,
            existing_completion_port: HANDLE,
            completion_key: usize,
            number_of_concurrent_threads: u32,
        ) -> HANDLE;
        pub fn GetQueuedCompletionStatusEx(
            completion_port: HANDLE,
            completion_port_entries: *mut OVERLAPPED_ENTRY,
            ul_count: u32,
            ul_num_entries_removed: *mut u32,
            dw_milliseconds: u32,
            alertable: i32,
        ) -> i32;
        pub fn PostQueuedCompletionStatus(
            completion_port: HANDLE,
            number_of_bytes_transferred: u32,
            completion_key: usize,
            overlapped: *mut OVERLAPPED,
        ) -> i32;
    }
}

#[cfg(not(windows))]
pub mod io {
    use super::foundation::HANDLE;

    #[repr(C)]
    pub struct OVERLAPPED {
        pub Internal: usize,
        pub InternalHigh: usize,
        pub Anonymous: OVERLAPPED_Union,
        pub hEvent: HANDLE,
    }

    #[repr(C)]
    pub union OVERLAPPED_Union {
        pub Pointer: *mut core::ffi::c_void,
        pub Offset: OVERLAPPED_Offset,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct OVERLAPPED_Offset {
        pub Offset: u32,
        pub OffsetHigh: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct OVERLAPPED_ENTRY {
        pub lpCompletionKey: usize,
        pub lpOverlapped: *mut OVERLAPPED,
        pub Internal: usize,
        pub dwNumberOfBytesTransferred: u32,
    }

    #[inline(always)]
    pub unsafe fn CancelIoEx(_file_handle: HANDLE, _lp_overlapped: *mut OVERLAPPED) -> i32 {
        panic!("CancelIoEx is only available on Windows targets");
    }

    #[inline(always)]
    pub unsafe fn CreateIoCompletionPort(
        _file_handle: HANDLE,
        _existing_completion_port: HANDLE,
        _completion_key: usize,
        _number_of_concurrent_threads: u32,
    ) -> HANDLE {
        panic!("CreateIoCompletionPort is only available on Windows targets");
    }

    #[inline(always)]
    pub unsafe fn GetQueuedCompletionStatusEx(
        _completion_port: HANDLE,
        _completion_port_entries: *mut OVERLAPPED_ENTRY,
        _ul_count: u32,
        _ul_num_entries_removed: *mut u32,
        _dw_milliseconds: u32,
        _alertable: i32,
    ) -> i32 {
        panic!("GetQueuedCompletionStatusEx is only available on Windows targets");
    }

    #[inline(always)]
    pub unsafe fn PostQueuedCompletionStatus(
        _completion_port: HANDLE,
        _number_of_bytes_transferred: u32,
        _completion_key: usize,
        _overlapped: *mut OVERLAPPED,
    ) -> i32 {
        panic!("PostQueuedCompletionStatus is only available on Windows targets");
    }
}

#[cfg(windows)]
pub mod file_system {
    use super::foundation::HANDLE;
    use super::io::OVERLAPPED;
    use core::ffi::c_void;

    pub const FILE_LIST_DIRECTORY: u32 = 0x0001;
    pub const FILE_SHARE_READ: u32 = 0x0000_0001;
    pub const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    pub const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    pub const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    pub const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    pub const FILE_ACTION_ADDED: u32 = 0x0000_0001;
    pub const FILE_ACTION_REMOVED: u32 = 0x0000_0002;
    pub const FILE_ACTION_MODIFIED: u32 = 0x0000_0003;
    pub const FILE_ACTION_RENAMED_OLD_NAME: u32 = 0x0000_0004;
    pub const FILE_ACTION_RENAMED_NEW_NAME: u32 = 0x0000_0005;
    pub const FILE_NOTIFY_CHANGE_FILE_NAME: u32 = 0x0000_0001;
    pub const FILE_NOTIFY_CHANGE_DIR_NAME: u32 = 0x0000_0002;
    pub const FILE_NOTIFY_CHANGE_ATTRIBUTES: u32 = 0x0000_0004;
    pub const FILE_NOTIFY_CHANGE_SIZE: u32 = 0x0000_0008;
    pub const FILE_NOTIFY_CHANGE_LAST_WRITE: u32 = 0x0000_0010;
    pub const FILE_NOTIFY_CHANGE_CREATION: u32 = 0x0000_0040;
    pub const FILE_NOTIFY_CHANGE_SECURITY: u32 = 0x0000_0100;
    pub const OPEN_EXISTING: u32 = 3;

    #[repr(C)]
    pub struct FILE_NOTIFY_INFORMATION {
        pub NextEntryOffset: u32,
        pub Action: u32,
        pub FileNameLength: u32,
        pub FileName: [u16; 1],
    }

    pub type LPOVERLAPPED_COMPLETION_ROUTINE =
        Option<unsafe extern "system" fn(u32, u32, *mut OVERLAPPED) -> ()>;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: HANDLE,
        ) -> HANDLE;
        pub fn ReadDirectoryChangesW(
            directory: HANDLE,
            buffer: *mut c_void,
            buffer_length: u32,
            watch_subtree: i32,
            notify_filter: u32,
            bytes_returned: *mut u32,
            overlapped: *mut OVERLAPPED,
            completion_routine: LPOVERLAPPED_COMPLETION_ROUTINE,
        ) -> i32;
    }
}

#[cfg(not(windows))]
pub mod file_system {
    use super::foundation::HANDLE;
    use super::io::OVERLAPPED;

    pub const FILE_LIST_DIRECTORY: u32 = 0x0001;
    pub const FILE_SHARE_READ: u32 = 0x0000_0001;
    pub const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    pub const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    pub const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    pub const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    pub const FILE_ACTION_ADDED: u32 = 0x0000_0001;
    pub const FILE_ACTION_REMOVED: u32 = 0x0000_0002;
    pub const FILE_ACTION_MODIFIED: u32 = 0x0000_0003;
    pub const FILE_ACTION_RENAMED_OLD_NAME: u32 = 0x0000_0004;
    pub const FILE_ACTION_RENAMED_NEW_NAME: u32 = 0x0000_0005;
    pub const FILE_NOTIFY_CHANGE_FILE_NAME: u32 = 0x0000_0001;
    pub const FILE_NOTIFY_CHANGE_DIR_NAME: u32 = 0x0000_0002;
    pub const FILE_NOTIFY_CHANGE_ATTRIBUTES: u32 = 0x0000_0004;
    pub const FILE_NOTIFY_CHANGE_SIZE: u32 = 0x0000_0008;
    pub const FILE_NOTIFY_CHANGE_LAST_WRITE: u32 = 0x0000_0010;
    pub const FILE_NOTIFY_CHANGE_CREATION: u32 = 0x0000_0040;
    pub const FILE_NOTIFY_CHANGE_SECURITY: u32 = 0x0000_0100;
    pub const OPEN_EXISTING: u32 = 3;

    #[repr(C)]
    pub struct FILE_NOTIFY_INFORMATION {
        pub NextEntryOffset: u32,
        pub Action: u32,
        pub FileNameLength: u32,
        pub FileName: [u16; 1],
    }

    #[inline(always)]
    pub unsafe fn CreateFileW(
        _file_name: *const u16,
        _desired_access: u32,
        _share_mode: u32,
        _security_attributes: *mut core::ffi::c_void,
        _creation_disposition: u32,
        _flags_and_attributes: u32,
        _template_file: HANDLE,
    ) -> HANDLE {
        panic!("CreateFileW is only available on Windows targets");
    }

    #[inline(always)]
    pub unsafe fn ReadDirectoryChangesW(
        _directory: HANDLE,
        _buffer: *mut core::ffi::c_void,
        _buffer_length: u32,
        _watch_subtree: i32,
        _notify_filter: u32,
        _bytes_returned: *mut u32,
        _overlapped: *mut OVERLAPPED,
        _completion_routine: Option<unsafe extern "system" fn(u32, u32, *mut OVERLAPPED) -> ()>,
    ) -> i32 {
        panic!("ReadDirectoryChangesW is only available on Windows targets");
    }
}

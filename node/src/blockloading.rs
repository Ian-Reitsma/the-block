use std::{
    ffi::{CStr, CString},
    os::raw::{c_char, c_int, c_void},
    path::Path,
    ptr,
};

/// Represents an in-house dynamic library loader.
pub struct BlockLibrary {
    handle: *mut c_void,
}

impl BlockLibrary {
    /// Open a dynamic library from the given path.
    pub fn open(path: impl AsRef<Path>) -> Option<Self> {
        let path_ref = path.as_ref();
        let c_path = CString::new(path_ref.as_os_str().as_bytes()).ok()?;
        let handle = unsafe { platform::open(c_path.as_ptr()) };
        if handle.is_null() {
            log::warn!(
                "blockloading: failed to open {:?}: {:?}",
                path_ref,
                platform::last_error()
            );
            return None;
        }
        Some(Self { handle })
    }

    /// Fetch a typed symbol from the library.
    pub fn symbol<T>(&self, name: &str) -> Option<T>
    where
        T: Copy,
    {
        let c_name = CString::new(name).ok()?;
        let ptr = unsafe { platform::symbol(self.handle, c_name.as_ptr()) };
        if ptr.is_null() {
            log::warn!(
                "blockloading: missing symbol {}: {:?}",
                name,
                platform::last_error()
            );
            return None;
        }
        Some(unsafe { std::mem::transmute(ptr) })
    }
}

impl Drop for BlockLibrary {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                platform::close(self.handle);
            }
        }
    }
}

mod platform {
    use super::*;

    #[cfg(unix)]
    pub unsafe fn open(path: *const c_char) -> *mut c_void {
        const RTLD_NOW: c_int = 2;
        dlopen(path, RTLD_NOW)
    }

    #[cfg(unix)]
    pub unsafe fn symbol(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
        dlsym(handle, symbol)
    }

    #[cfg(unix)]
    pub unsafe fn close(handle: *mut c_void) -> c_int {
        dlclose(handle)
    }

    #[cfg(unix)]
    pub fn last_error() -> Option<String> {
        unsafe {
            let err = dlerror();
            if err.is_null() {
                None
            } else {
                CStr::from_ptr(err).to_string_lossy().into_owned().into()
            }
        }
    }

    #[cfg(windows)]
    pub unsafe fn open(path: *const c_char) -> *mut c_void {
        LoadLibraryA(path) as *mut c_void
    }

    #[cfg(windows)]
    pub unsafe fn symbol(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
        GetProcAddress(handle as *mut _, symbol) as *mut c_void
    }

    #[cfg(windows)]
    pub unsafe fn close(handle: *mut c_void) -> c_int {
        FreeLibrary(handle as *mut _);
        0
    }

    #[cfg(windows)]
    pub fn last_error() -> Option<String> {
        None
    }

    #[cfg(unix)]
    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    #[cfg(windows)]
    extern "system" {
        fn LoadLibraryA(lpLibFileName: *const c_char) -> *mut c_void;
        fn GetProcAddress(hModule: *mut c_void, lpProcName: *const c_char) -> *mut c_void;
        fn FreeLibrary(hLibModule: *mut c_void) -> i32;
    }
}

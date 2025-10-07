use std::ffi::{c_void, CStr, CString};
use std::mem;
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::{Error, Result};

pub struct SharedLibrary {
    handle: Handle,
}

impl SharedLibrary {
    pub fn load(path: &str) -> Result<Self> {
        let handle = platform::open(path)?;
        Ok(Self { handle })
    }

    pub fn get<T>(&self, symbol: &[u8]) -> Result<T>
    where
        T: Copy,
    {
        platform::get(&self.handle, symbol)
    }
}

impl Drop for SharedLibrary {
    fn drop(&mut self) {
        platform::close(&mut self.handle);
    }
}

struct Handle(platform::HandleRaw);

unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

unsafe impl Send for SharedLibrary {}
unsafe impl Sync for SharedLibrary {}

mod platform {
    use super::*;

    #[cfg(unix)]
    pub type HandleRaw = *mut c_void;

    #[cfg(windows)]
    pub type HandleRaw = *mut c_void;

    #[cfg(not(any(unix, windows)))]
    pub type HandleRaw = ();

    pub fn open(path: &str) -> Result<Handle> {
        let c_path = CString::new(path).map_err(|_| {
            Error::value(format!("library path contains interior NUL bytes: {path}"))
        })?;
        let raw = unsafe { open_raw(&c_path)? };
        Ok(Handle(raw))
    }

    pub fn get<T>(handle: &Handle, symbol: &[u8]) -> Result<T>
    where
        T: Copy + Sized,
    {
        let c_symbol = CStr::from_bytes_with_nul(symbol)
            .map_err(|_| Error::value("symbol names must be NUL-terminated"))?;
        let ptr = unsafe { get_raw(handle.0, c_symbol)? };
        if ptr.is_null() {
            return Err(Error::runtime("resolved symbol is null"));
        }
        Ok(unsafe { mem::transmute_copy(&ptr) })
    }

    pub fn close(handle: &mut Handle) {
        unsafe { close_raw(handle.0) };
        handle.0 = default_raw();
    }

    #[cfg(unix)]
    unsafe fn open_raw(path: &CString) -> Result<HandleRaw> {
        const RTLD_NOW: c_int = 2;
        extern "C" {
            fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        }
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(Error::runtime(format!(
                "dlopen failed for {}: {}",
                path.to_string_lossy(),
                last_error()
            )));
        }
        Ok(handle)
    }

    #[cfg(unix)]
    unsafe fn get_raw(handle: HandleRaw, symbol: &CStr) -> Result<*mut c_void> {
        extern "C" {
            fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
            fn dlerror() -> *const c_char;
        }
        // Clear any stale error before calling into dlsym so we can surface
        // the precise failure if symbol resolution misses.
        let _ = unsafe { dlerror() };
        let ptr = unsafe { dlsym(handle, symbol.as_ptr()) };
        if ptr.is_null() {
            return Err(Error::runtime(format!(
                "dlsym failed for {}: {}",
                symbol.to_string_lossy(),
                last_error()
            )));
        }
        Ok(ptr)
    }

    #[cfg(unix)]
    unsafe fn close_raw(handle: HandleRaw) {
        extern "C" {
            fn dlclose(handle: *mut c_void) -> c_int;
        }
        if !handle.is_null() {
            let _ = unsafe { dlclose(handle) };
        }
    }

    #[cfg(unix)]
    fn default_raw() -> HandleRaw {
        ptr::null_mut()
    }

    #[cfg(unix)]
    fn last_error() -> String {
        extern "C" {
            fn dlerror() -> *const c_char;
        }
        unsafe {
            let ptr = dlerror();
            if ptr.is_null() {
                "unknown error".to_string()
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        }
    }

    #[cfg(windows)]
    unsafe fn open_raw(path: &CString) -> Result<HandleRaw> {
        use std::os::raw::c_char;
        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryA(name: *const c_char) -> *mut c_void;
            fn GetLastError() -> u32;
        }
        let handle = unsafe { LoadLibraryA(path.as_ptr()) };
        if handle.is_null() {
            return Err(Error::runtime(format!(
                "LoadLibraryA failed for {}: error {:#x}",
                path.to_string_lossy(),
                unsafe { GetLastError() }
            )));
        }
        Ok(handle)
    }

    #[cfg(windows)]
    unsafe fn get_raw(handle: HandleRaw, symbol: &CStr) -> Result<*mut c_void> {
        use std::os::raw::c_char;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetProcAddress(handle: *mut c_void, name: *const c_char) -> *mut c_void;
            fn GetLastError() -> u32;
        }
        let ptr = unsafe { GetProcAddress(handle, symbol.as_ptr()) };
        if ptr.is_null() {
            return Err(Error::runtime(format!(
                "GetProcAddress failed for {}: error {:#x}",
                symbol.to_string_lossy(),
                unsafe { GetLastError() }
            )));
        }
        Ok(ptr)
    }

    #[cfg(windows)]
    unsafe fn close_raw(handle: HandleRaw) {
        #[link(name = "kernel32")]
        extern "system" {
            fn FreeLibrary(handle: *mut c_void) -> i32;
        }
        if !handle.is_null() {
            let _ = unsafe { FreeLibrary(handle) };
        }
    }

    #[cfg(windows)]
    fn default_raw() -> HandleRaw {
        ptr::null_mut()
    }

    #[cfg(not(any(unix, windows)))]
    unsafe fn open_raw(_path: &CString) -> Result<HandleRaw> {
        Err(Error::unimplemented(
            "dynamic library loading is not supported on this platform",
        ))
    }

    #[cfg(not(any(unix, windows)))]
    unsafe fn get_raw(_handle: HandleRaw, _symbol: &CStr) -> Result<*mut c_void> {
        Err(Error::unimplemented(
            "dynamic library loading is not supported on this platform",
        ))
    }

    #[cfg(not(any(unix, windows)))]
    unsafe fn close_raw(_handle: HandleRaw) {}

    #[cfg(not(any(unix, windows)))]
    fn default_raw() -> HandleRaw {
        ()
    }
}

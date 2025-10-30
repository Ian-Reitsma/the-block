#![deny(unsafe_op_in_unsafe_fn)]

use std::env;
use std::ffi::{c_void, CString};
use std::fmt;
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::slice;
use std::str;
use std::sync::OnceLock;

#[cfg(feature = "python-bindings")]
pub use python_bridge_macros::{getter, new, setter, staticmethod};

mod loader;
use loader::SharedLibrary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    FeatureDisabled,
    Unimplemented,
    Runtime,
    Value,
}

#[derive(Debug, Clone)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    pub fn feature_disabled() -> Self {
        Self {
            kind: ErrorKind::FeatureDisabled,
            message: "python bindings are disabled".to_string(),
        }
    }

    pub fn unimplemented(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Unimplemented,
            message: msg.into(),
        }
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Runtime,
            message: msg.into(),
        }
    }

    pub fn value(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Value,
            message: msg.into(),
        }
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn with_message(mut self, msg: impl Into<String>) -> Self {
        self.message = msg.into();
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Guard object that provides access to the active Python interpreter while the
/// global interpreter lock (GIL) is held.
pub struct Interpreter<'a> {
    runtime: &'a PythonRuntime,
    _gil: GILGuard<'a>,
}

impl<'a> Interpreter<'a> {
    /// Execute the supplied Python source string. Any uncaught Python
    /// exceptions are printed via `PyErr_Print` and surfaced as a runtime
    /// error.
    pub fn run(&self, code: &str) -> Result<()> {
        let cstr = CString::new(code)
            .map_err(|_| Error::value("python source cannot contain NUL bytes"))?;
        unsafe {
            let status = match &self.runtime.runner {
                RunFunction::Flags(func) => func(cstr.as_ptr(), std::ptr::null_mut()),
                RunFunction::Simple(func) => func(cstr.as_ptr()),
            };
            if status != 0 {
                (self.runtime.err_print)();
                return Err(Error::runtime("python execution failed"));
            }
        }
        Ok(())
    }

    /// Import `module`, look up `function`, and invoke it with the provided
    /// string arguments. The Python function's string representation is
    /// returned when it evaluates to a non-``None`` object.
    pub fn call_function(
        &self,
        module: &str,
        function: &str,
        args: &[&str],
    ) -> Result<Option<String>> {
        self.runtime.call_function(module, function, args)
    }
}

/// Ensure that the Python runtime is available and initialised.
pub fn ensure_enabled() -> Result<()> {
    runtime().map(|_| ())
}

/// Initialise the embedded interpreter in a free-threaded configuration. This
/// currently forwards to [`ensure_enabled`].
pub fn prepare_freethreaded_python() -> Result<()> {
    ensure_enabled()
}

/// Execute a closure while holding the Python GIL. The closure receives an
/// [`Interpreter`] handle that exposes minimal helpers for executing source
/// strings.
pub fn with_interpreter<F, T>(func: F) -> Result<T>
where
    F: FnOnce(&Interpreter<'_>) -> Result<T>,
{
    let runtime = runtime()?;
    let guard = runtime.acquire_gil();
    let interpreter = Interpreter {
        runtime,
        _gil: guard,
    };
    func(&interpreter)
}

pub fn report_disabled() -> Error {
    Error::feature_disabled()
}

static RUNTIME: OnceLock<Result<PythonRuntime>> = OnceLock::new();

fn runtime() -> Result<&'static PythonRuntime> {
    match RUNTIME.get_or_init(PythonRuntime::load) {
        Ok(runtime) => Ok(runtime),
        Err(err) => Err(err.clone()),
    }
}

struct PythonRuntime {
    _lib: SharedLibrary,
    initialize: unsafe extern "C" fn(),
    is_initialized: unsafe extern "C" fn() -> c_int,
    gil_ensure: unsafe extern "C" fn() -> c_int,
    gil_release: unsafe extern "C" fn(c_int),
    runner: RunFunction,
    err_print: unsafe extern "C" fn(),
    import_module: unsafe extern "C" fn(*const c_char) -> *mut PyObject,
    get_attr_string: unsafe extern "C" fn(*mut PyObject, *const c_char) -> *mut PyObject,
    tuple_new: unsafe extern "C" fn(isize) -> *mut PyObject,
    tuple_set_item: unsafe extern "C" fn(*mut PyObject, isize, *mut PyObject) -> c_int,
    unicode_from_string_and_size: unsafe extern "C" fn(*const c_char, isize) -> *mut PyObject,
    object_call_object: unsafe extern "C" fn(*mut PyObject, *mut PyObject) -> *mut PyObject,
    object_str: unsafe extern "C" fn(*mut PyObject) -> *mut PyObject,
    unicode_as_utf8_and_size: unsafe extern "C" fn(*mut PyObject, *mut isize) -> *const c_char,
    dec_ref: unsafe extern "C" fn(*mut PyObject),
    none: *mut PyObject,
}

unsafe impl Send for PythonRuntime {}
unsafe impl Sync for PythonRuntime {}

enum RunFunction {
    Flags(unsafe extern "C" fn(*const c_char, *mut PyCompilerFlags) -> c_int),
    Simple(unsafe extern "C" fn(*const c_char) -> c_int),
}

#[repr(C)]
struct PyCompilerFlags {
    cf_flags: c_int,
}

struct GILGuard<'a> {
    runtime: &'a PythonRuntime,
    state: c_int,
}

impl<'a> Drop for GILGuard<'a> {
    fn drop(&mut self) {
        unsafe {
            (self.runtime.gil_release)(self.state);
        }
    }
}

impl PythonRuntime {
    fn load() -> Result<PythonRuntime> {
        let lib = load_library()?;
        let runtime = {
            let initialize: unsafe extern "C" fn() = lib.get(b"Py_Initialize\0")?;
            let is_initialized: unsafe extern "C" fn() -> c_int = lib.get(b"Py_IsInitialized\0")?;
            let gil_ensure: unsafe extern "C" fn() -> c_int = lib.get(b"PyGILState_Ensure\0")?;
            let gil_release: unsafe extern "C" fn(c_int) = lib.get(b"PyGILState_Release\0")?;

            let runner = match lib
                .get::<unsafe extern "C" fn(*const c_char, *mut PyCompilerFlags) -> c_int>(
                    b"PyRun_SimpleStringFlags\0",
                ) {
                Ok(symbol) => RunFunction::Flags(symbol),
                Err(_) => {
                    let simple = lib.get::<unsafe extern "C" fn(*const c_char) -> c_int>(
                        b"PyRun_SimpleString\0",
                    )?;
                    RunFunction::Simple(simple)
                }
            };

            let import_module: unsafe extern "C" fn(*const c_char) -> *mut PyObject =
                lib.get(b"PyImport_ImportModule\0")?;
            let get_attr_string: unsafe extern "C" fn(
                *mut PyObject,
                *const c_char,
            ) -> *mut PyObject = lib.get(b"PyObject_GetAttrString\0")?;
            let tuple_new: unsafe extern "C" fn(isize) -> *mut PyObject =
                lib.get(b"PyTuple_New\0")?;
            let tuple_set_item: unsafe extern "C" fn(*mut PyObject, isize, *mut PyObject) -> c_int =
                lib.get(b"PyTuple_SetItem\0")?;
            let unicode_from_string_and_size: unsafe extern "C" fn(
                *const c_char,
                isize,
            ) -> *mut PyObject = lib.get(b"PyUnicode_FromStringAndSize\0")?;
            let object_call_object: unsafe extern "C" fn(
                *mut PyObject,
                *mut PyObject,
            ) -> *mut PyObject = lib.get(b"PyObject_CallObject\0")?;
            let object_str: unsafe extern "C" fn(*mut PyObject) -> *mut PyObject =
                lib.get(b"PyObject_Str\0")?;
            let unicode_as_utf8_and_size: unsafe extern "C" fn(
                *mut PyObject,
                *mut isize,
            ) -> *const c_char = lib.get(b"PyUnicode_AsUTF8AndSize\0")?;
            let dec_ref: unsafe extern "C" fn(*mut PyObject) = lib.get(b"Py_DecRef\0")?;
            let none: *mut PyObject = lib.get(b"_Py_NoneStruct\0")?;

            let err_print: unsafe extern "C" fn() = lib.get(b"PyErr_Print\0")?;

            PythonRuntime {
                _lib: lib,
                initialize,
                is_initialized,
                gil_ensure,
                gil_release,
                runner,
                err_print,
                import_module,
                get_attr_string,
                tuple_new,
                tuple_set_item,
                unicode_from_string_and_size,
                object_call_object,
                object_str,
                unicode_as_utf8_and_size,
                dec_ref,
                none,
            }
        };

        if unsafe { (runtime.is_initialized)() } == 0 {
            unsafe { (runtime.initialize)() };
        }

        Ok(runtime)
    }

    fn acquire_gil(&'static self) -> GILGuard<'static> {
        let state = unsafe { (self.gil_ensure)() };
        GILGuard {
            runtime: self,
            state,
        }
    }

    fn call_function(&self, module: &str, function: &str, args: &[&str]) -> Result<Option<String>> {
        let module_name = CString::new(module)
            .map_err(|_| Error::value("module names cannot contain interior NUL bytes"))?;
        let module_obj = unsafe { (self.import_module)(module_name.as_ptr()) };
        if module_obj.is_null() {
            unsafe { (self.err_print)() };
            return Err(Error::runtime(format!(
                "failed to import python module {module}"
            )));
        }
        let module_handle = OwnedPyObject::new(module_obj, self);

        let func_name = CString::new(function)
            .map_err(|_| Error::value("function names cannot contain interior NUL bytes"))?;
        let func_obj = unsafe { (self.get_attr_string)(module_handle.ptr, func_name.as_ptr()) };
        if func_obj.is_null() {
            unsafe { (self.err_print)() };
            return Err(Error::runtime(format!(
                "python module {module} has no attribute {function}"
            )));
        }
        let function_handle = OwnedPyObject::new(func_obj, self);

        let tuple_ptr = unsafe { (self.tuple_new)(args.len() as isize) };
        if tuple_ptr.is_null() {
            return Err(Error::runtime("failed to allocate python argument tuple"));
        }
        let args_tuple = OwnedPyObject::new(tuple_ptr, self);

        for (index, value) in args.iter().enumerate() {
            let bytes = value.as_bytes();
            let py_obj = unsafe {
                (self.unicode_from_string_and_size)(
                    bytes.as_ptr() as *const c_char,
                    bytes.len() as isize,
                )
            };
            if py_obj.is_null() {
                unsafe { (self.err_print)() };
                return Err(Error::runtime(format!(
                    "failed to convert argument {index} to python string"
                )));
            }
            // `PyTuple_SetItem` steals a reference to `py_obj` on success. When an
            // error occurs it does **not** steal, so we must balance refcounts.
            let rc = unsafe { (self.tuple_set_item)(args_tuple.ptr, index as isize, py_obj) };
            if rc != 0 {
                unsafe {
                    (self.dec_ref)(py_obj);
                    (self.err_print)();
                }
                return Err(Error::runtime("failed to populate python argument tuple"));
            }
        }

        let result_ptr = unsafe { (self.object_call_object)(function_handle.ptr, args_tuple.ptr) };
        if result_ptr.is_null() {
            unsafe { (self.err_print)() };
            return Err(Error::runtime(format!(
                "python call {module}.{function} raised an exception"
            )));
        }
        let result = OwnedPyObject::new(result_ptr, self);

        if ptr::eq(result.ptr, self.none) {
            return Ok(None);
        }

        let str_ptr = unsafe { (self.object_str)(result.ptr) };
        if str_ptr.is_null() {
            unsafe { (self.err_print)() };
            return Err(Error::runtime("failed to stringify python return value"));
        }
        let str_obj = OwnedPyObject::new(str_ptr, self);

        let mut out_len: isize = 0;
        let data_ptr = unsafe { (self.unicode_as_utf8_and_size)(str_obj.ptr, &mut out_len) };
        if data_ptr.is_null() {
            unsafe { (self.err_print)() };
            return Err(Error::runtime(
                "python returned a non-UTF-8 result that cannot be represented in Rust",
            ));
        }

        let bytes = unsafe { slice::from_raw_parts(data_ptr as *const u8, out_len as usize) };
        let string = str::from_utf8(bytes)
            .map_err(|_| Error::runtime("python returned non-UTF-8 data"))?
            .to_owned();
        Ok(Some(string))
    }
}

type PyObject = c_void;

struct OwnedPyObject<'py> {
    ptr: *mut PyObject,
    runtime: &'py PythonRuntime,
}

impl<'py> OwnedPyObject<'py> {
    fn new(ptr: *mut PyObject, runtime: &'py PythonRuntime) -> Self {
        Self { ptr, runtime }
    }
}

impl<'py> Drop for OwnedPyObject<'py> {
    fn drop(&mut self) {
        unsafe { (self.runtime.dec_ref)(self.ptr) };
    }
}

fn load_library() -> Result<SharedLibrary> {
    if let Ok(path) = env::var("PYTHON_BRIDGE_LIB") {
        return SharedLibrary::load(&path).map_err(|err| {
            let mut message = format!("failed to load python library from {path}: ");
            message.push_str(err.message());
            err.with_message(message)
        });
    }

    for candidate in default_library_candidates() {
        if let Ok(lib) = SharedLibrary::load(candidate) {
            return Ok(lib);
        }
    }

    Err(Error::runtime(
        "unable to locate libpython; set PYTHON_BRIDGE_LIB to the shared library path",
    ))
}

fn default_library_candidates() -> &'static [&'static str] {
    #[cfg(target_os = "linux")]
    {
        &[
            "libpython3.12.so",
            "libpython3.12m.so",
            "libpython3.11.so",
            "libpython3.11m.so",
            "libpython3.10.so",
            "libpython3.10m.so",
            "libpython3.9.so",
            "libpython3.9m.so",
            "libpython3.8.so",
            "libpython3.8m.so",
            "libpython3.so",
        ]
    }
    #[cfg(target_os = "macos")]
    {
        &[
            "libpython3.12.dylib",
            "libpython3.11.dylib",
            "libpython3.10.dylib",
            "libpython3.9.dylib",
            "libpython3.8.dylib",
            "Python3",
        ]
    }
    #[cfg(target_os = "windows")]
    {
        &[
            "python312.dll",
            "python311.dll",
            "python310.dll",
            "python39.dll",
            "python38.dll",
            "python3.dll",
        ]
    }
    #[cfg(all(
        not(target_os = "linux"),
        not(target_os = "macos"),
        not(target_os = "windows")
    ))]
    {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn runtime_available() -> bool {
        match ensure_enabled() {
            Ok(()) => true,
            Err(err) if err.kind() == &ErrorKind::Runtime => {
                eprintln!("python runtime unavailable for tests: {}", err.message());
                false
            }
            Err(err) => panic!("unexpected python bridge error: {err}"),
        }
    }

    fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("test lock")
    }

    #[test]
    fn python_runtime_behaviour() {
        let _guard = serial_guard();
        if !runtime_available() {
            return;
        }

        with_interpreter(|py| {
            py.run("import sys\nassert sys.version_info.major >= 3")?;
            let err = py.run("raise RuntimeError('boom')");
            assert!(matches!(err, Err(e) if e.kind() == &ErrorKind::Runtime));

            py.run(
                r#"
def _tb_add(a, b):
    return int(a) + int(b)

def _tb_none():
    return None
"#,
            )?;
            let value = py
                .call_function("__main__", "_tb_add", &["2", "40"])?
                .expect("result present");
            assert_eq!(value, "42");

            let none = py.call_function("__main__", "_tb_none", &[])?;
            assert!(none.is_none());

            let err = py
                .call_function("__main__", "_tb_missing", &[])
                .expect_err("missing function should error");
            assert_eq!(err.kind(), &ErrorKind::Runtime);
            Ok(())
        })
        .expect("python interactions to succeed");
    }
}

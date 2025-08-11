use std::fs;

use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

use the_block::{maybe_spawn_purge_loop_py, Blockchain, ShutdownFlag};

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

fn unique_path(prefix: &str) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let id = COUNT.fetch_add(1, AtomicOrdering::Relaxed);
    format!("{prefix}_{id}")
}

#[test]
fn purge_loop_join_surfaces_panic() {
    init();
    let path = unique_path("purge_loop_join_panic");
    let _ = fs::remove_dir_all(&path);
    std::env::set_var("TB_PURGE_LOOP_SECS", "1");
    let bc = Blockchain::open(&path).unwrap();
    bc.panic_next_purge();
    let shutdown = ShutdownFlag::new();

    let result = Python::with_gil(|py| {
        let bc_py = Py::new(py, bc).unwrap();
        let handle =
            maybe_spawn_purge_loop_py(bc_py.clone_ref(py), &shutdown).expect("loop not started");
        let handle_py = Py::new(py, handle).unwrap();
        let code = r#"
import time

def trigger(handle):
    time.sleep(0.1)
    handle.join()
"#;
        let code_c = CString::new(code).unwrap();
        let filename = CString::new("purge_loop_panic.py").unwrap();
        let module_name = CString::new("purge_loop_panic").unwrap();
        let module = PyModule::from_code(
            py,
            code_c.as_c_str(),
            filename.as_c_str(),
            module_name.as_c_str(),
        )?;
        let trigger = module.getattr("trigger")?;
        trigger.call1((handle_py,))?;
        Ok::<(), PyErr>(())
    });
    assert!(result.is_err());
    Python::with_gil(|py| {
        let err = result.unwrap_err();
        assert!(err.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
        let msg = err.value(py).to_string();
        assert!(msg.contains("purge panic"));
    });
    std::env::remove_var("TB_PURGE_LOOP_SECS");
}

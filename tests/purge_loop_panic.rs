use std::fs;

#[cfg(target_os = "linux")]
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

mod util;
use util::temp::temp_dir;

use serial_test::serial;
use the_block::{maybe_spawn_purge_loop_py, Blockchain, ShutdownFlag};

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

fn run_purge_panic(backtrace: bool) -> String {
    let dir = temp_dir("purge_loop_join_panic");
    std::env::set_var("TB_PURGE_LOOP_SECS", "1");
    if backtrace {
        std::env::set_var("RUST_BACKTRACE", "1");
    } else {
        std::env::remove_var("RUST_BACKTRACE");
    }
    let bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
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
    let msg = Python::with_gil(|py| {
        let err = result.unwrap_err();
        assert!(err.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
        err.value(py).to_string()
    });
    std::env::remove_var("TB_PURGE_LOOP_SECS");
    if backtrace {
        std::env::remove_var("RUST_BACKTRACE");
    }
    msg
}

#[test]
#[serial]
fn purge_loop_join_surfaces_panic() {
    init();
    let msg = run_purge_panic(false);
    assert!(msg.contains("purge panic"));
    assert!(!msg.contains("Backtrace"));
    let msg_bt = run_purge_panic(true);
    assert!(msg_bt.contains("purge panic"));
    assert!(msg_bt.contains("Backtrace"));
}

#[cfg(target_os = "linux")]
fn thread_count() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().count()
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn purge_loop_joins_on_drop() {
    init();
    let dir = temp_dir("purge_loop_drop_join");
    std::env::set_var("TB_PURGE_LOOP_SECS", "1");
    let bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    let shutdown = ShutdownFlag::new();

    let before = thread_count();
    let handle = Python::with_gil(|py| {
        let bc_py = Py::new(py, bc).unwrap();
        maybe_spawn_purge_loop_py(bc_py, &shutdown).expect("loop not started")
    });

    let mut mid = thread_count();
    // Allow up to ~5s for the purge loop thread to spawn.
    for _ in 0..500 {
        if mid > before {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        mid = thread_count();
    }
    assert!(mid > before);

    shutdown.trigger();
    drop(handle);

    let mut after = thread_count();
    // Wait up to ~5s for the thread to terminate after dropping the handle.
    for _ in 0..500 {
        if after == before {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        after = thread_count();
    }
    assert!(after <= before);

    std::env::remove_var("TB_PURGE_LOOP_SECS");
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn purge_loop_drop_without_trigger_stops_thread() {
    init();
    let dir = temp_dir("purge_loop_drop_no_trigger");
    std::env::set_var("TB_PURGE_LOOP_SECS", "1");
    let bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    let shutdown = ShutdownFlag::new();

    let before = thread_count();
    let handle = Python::with_gil(|py| {
        let bc_py = Py::new(py, bc).unwrap();
        maybe_spawn_purge_loop_py(bc_py, &shutdown).expect("loop not started")
    });

    let mut mid = thread_count();
    // Allow extra time for the purge loop thread to spawn under load.
    for _ in 0..500 {
        if mid > before {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        mid = thread_count();
    }
    assert!(mid > before);

    drop(handle);

    let mut after = thread_count();
    for _ in 0..500 {
        if after == before {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        after = thread_count();
    }
    assert!(after <= before);

    std::env::remove_var("TB_PURGE_LOOP_SECS");
}

#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use std::fs;
use std::thread;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

mod util;
use util::temp::temp_dir;

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

#[test]
fn purge_loop_shutdowns_on_exception() {
    init();
    let dir = temp_dir("purge_loop_exception");
    std::env::set_var("TB_PURGE_LOOP_SECS", "1");
    let mut bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    bc.min_fee_per_byte_consumer = 0;
    bc.min_fee_per_byte_industrial = 0;
    bc.base_fee = 0;
    bc.add_account("a".into(), 10, 10).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        pct_ct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    bc.submit_transaction(tx).unwrap();
    let key = (String::from("a"), 1u64);
    if let Some(mut entry) = bc.mempool_consumer.get_mut(&key) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    };
    bc.tx_ttl = 1;
    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();

    // move bc into Python
    let bc_py = Python::with_gil(|py| Py::new(py, bc).unwrap());

    let result: PyResult<()> = Python::with_gil(|py| {
        let code = r#"
import time, the_block

def boom(bc):
    with the_block.PurgeLoop(bc):
        time.sleep(1.1)
        raise RuntimeError('boom')
"#;
        let code_c = CString::new(code).unwrap();
        let filename = CString::new("purge_loop_exception.py").unwrap();
        let module_name = CString::new("purge_loop_exception").unwrap();
        let module = PyModule::from_code(
            py,
            code_c.as_c_str(),
            filename.as_c_str(),
            module_name.as_c_str(),
        )?;
        let boom = module.getattr("boom")?;
        boom.call1((bc_py.clone_ref(py),))?;
        Ok(())
    });
    assert!(result.is_err());

    // insert another expired transaction after the exception
    Python::with_gil(|py| {
        let mut bc_ref = bc_py.borrow_mut(py);
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1,
            pct_ct: 100,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc_ref.submit_transaction(tx).unwrap();
        let key = (String::from("a"), 1u64);
        if let Some(mut entry) = bc_ref.mempool_consumer.get_mut(&key) {
            entry.timestamp_millis = 0;
            entry.timestamp_ticks = 0;
        };
    });

    thread::sleep(Duration::from_millis(1100));

    #[cfg(feature = "telemetry")]
    assert_eq!(1, telemetry::TTL_DROP_TOTAL.get());

    Python::with_gil(|py| {
        let bc_ref = bc_py.borrow(py);
        assert_eq!(1, bc_ref.mempool_consumer.len());
    });

    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();
    std::env::remove_var("TB_PURGE_LOOP_SECS");
}

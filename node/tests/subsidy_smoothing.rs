#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::governance::{retune_multipliers, Params, Utilization};

#[test]
fn smoothing_limits_burst_effect() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    let mut params = Params {
        util_var_threshold: 100,
        fib_window_base_secs: 4,
        ..Default::default()
    };
    let supply = 1_000_000.0;
    let mut util = Utilization {
        bytes_stored: 1000.0,
        bytes_read: 0.0,
        cpu_ms: 0.0,
        bytes_out: 0.0,
        epoch_secs: 4.0,
    };
    // build history with steady usage
    for epoch in 0..5 {
        retune_multipliers(&mut params, supply, &util, epoch, base, 0.0, Some(0));
    }
    let prev = params.beta_storage_sub_ct;
    // bursty epoch
    util.bytes_stored = 1_000_000.0;
    retune_multipliers(&mut params, supply, &util, 5, base, 0.0, Some(0));
    assert!(params.beta_storage_sub_ct >= (prev as f64 * 0.85) as i64);
}

#[test]
fn steady_usage_stable_multiplier() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    let mut params = Params {
        util_var_threshold: 100,
        fib_window_base_secs: 4,
        ..Default::default()
    };
    let supply = 1_000_000.0;
    let util = Utilization {
        bytes_stored: 2000.0,
        bytes_read: 0.0,
        cpu_ms: 0.0,
        bytes_out: 0.0,
        epoch_secs: 4.0,
    };
    for epoch in 0..5 {
        retune_multipliers(&mut params, supply, &util, epoch, base, 0.0, Some(0));
    }
    let prev = params.beta_storage_sub_ct;
    retune_multipliers(&mut params, supply, &util, 5, base, 0.0, Some(0));
    // Allow a small integer jitter when utilisation remains steady.
    assert!((params.beta_storage_sub_ct - prev).abs() <= 3);
}

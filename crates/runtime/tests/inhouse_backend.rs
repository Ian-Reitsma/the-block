#![cfg(feature = "inhouse-backend")]
#![recursion_limit = "65536"]

use runtime;
use std::sync::Once;
use std::time::{Duration, Instant};

fn ensure_inhouse_backend() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("TB_RUNTIME_BACKEND", "inhouse");
        assert_eq!(runtime::handle().backend_name(), "inhouse");
    });
}

#[test]
fn spawn_executes_future_on_inhouse_runtime() {
    ensure_inhouse_backend();

    let result = runtime::block_on(async {
        let handle = runtime::spawn(async { 2_usize + 2 });
        handle.await.expect("task panicked")
    });

    assert_eq!(result, 4);
}

#[test]
fn spawn_blocking_runs_on_dedicated_thread() {
    ensure_inhouse_backend();

    let result = runtime::block_on(async {
        let handle = runtime::spawn_blocking(|| 6_i32 * 7);
        handle.await.expect("blocking task panicked")
    });

    assert_eq!(result, 42);
}

#[test]
fn sleep_delays_execution() {
    ensure_inhouse_backend();

    let elapsed = runtime::block_on(async {
        let start = Instant::now();
        runtime::sleep(Duration::from_millis(50)).await;
        start.elapsed()
    });

    assert!(elapsed >= Duration::from_millis(40));
}

#[test]
fn timeout_completes_successfully() {
    ensure_inhouse_backend();

    let value = runtime::block_on(async {
        runtime::timeout(Duration::from_millis(200), async { 123_u32 })
            .await
            .expect("timeout should not fire")
    });

    assert_eq!(value, 123_u32);
}

#[test]
fn timeout_expires_for_slow_future() {
    ensure_inhouse_backend();

    let err = runtime::block_on(async {
        runtime::timeout(Duration::from_millis(20), async {
            runtime::sleep(Duration::from_millis(200)).await;
        })
        .await
        .expect_err("timeout should elapse")
    });

    assert!(err.to_string().contains("timed out"));
}

#[test]
fn select_macro_observes_first_ready_branch() {
    ensure_inhouse_backend();

    let triggered = runtime::block_on(async {
        let short = runtime::sleep(Duration::from_millis(10));
        let long = runtime::sleep(Duration::from_millis(100));
        match runtime::select2(short, long).await {
            runtime::Either::First(()) => true,
            runtime::Either::Second(()) => false,
        }
    });

    assert!(triggered, "sleep branch should complete first");
}

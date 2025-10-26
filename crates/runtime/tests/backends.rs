#![allow(clippy::unwrap_used, clippy::expect_used)]

use runtime::{self, timeout};
use std::time::Duration;

#[test]
fn spawn_completes_future() {
    runtime::block_on(async {
        let handle = runtime::spawn(async { 41 + 1 });
        assert_eq!(handle.await.unwrap(), 42);
    });
}

#[test]
fn spawn_blocking_runs() {
    runtime::block_on(async {
        let handle = runtime::spawn_blocking(|| 21 * 2);
        assert_eq!(handle.await.unwrap(), 42);
    });
}

#[test]
fn timeout_expires() {
    runtime::block_on(async {
        let res = timeout(Duration::from_millis(5), async {
            runtime::sleep(Duration::from_millis(50)).await;
        })
        .await;
        assert!(res.is_err());
    });
}

#[test]
fn abort_cancels_task() {
    runtime::block_on(async {
        let handle = runtime::spawn(async {
            runtime::sleep(Duration::from_millis(100)).await;
            1
        });
        handle.abort();
        let err = handle.await.unwrap_err();
        assert!(err.is_cancelled());
    });
}

#[cfg(feature = "stub-backend")]
#[test]
fn stub_sleep_returns_promptly() {
    runtime::block_on(async {
        runtime::sleep(Duration::from_millis(1)).await;
    });
}

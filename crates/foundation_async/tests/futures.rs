use std::future::{ready, Future};
use std::thread;
use std::time::Duration;

use foundation_async::block_on;
use foundation_async::future::{catch_unwind, join_all, select2, Either};
use foundation_async::sync::oneshot;

#[test]
fn oneshot_channel_roundtrip() {
    let (tx, rx) = oneshot::channel();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        tx.send(42_u32).expect("send succeeds");
    });

    let received = block_on(async { rx.await.expect("value delivered") });
    assert_eq!(received, 42);
    handle.join().expect("sender thread joined");
}

#[test]
fn oneshot_channel_cancellation() {
    let (tx, rx) = oneshot::channel::<()>();
    drop(tx);
    let result = block_on(async { rx.await });
    assert!(result.is_err(), "receiver reports cancellation");
}

#[test]
fn join_all_preserves_order() {
    let futures = vec![ready(1_u8), ready(2_u8), ready(3_u8)];
    let outputs = block_on(async { join_all(futures).await });
    assert_eq!(outputs, vec![1, 2, 3]);
}

#[test]
fn join_all_empty_vector() {
    let futures: Vec<std::future::Ready<u8>> = Vec::new();
    let outputs = block_on(async { join_all(futures).await });
    assert!(outputs.is_empty());
}

#[test]
fn select2_prefers_first_ready_future() {
    let result = block_on(async { select2(async { 5 }, async { 9 }).await });
    match result {
        Either::First(value) => assert_eq!(value, 5),
        Either::Second(_) => panic!("expected first future to win"),
    }
}

#[test]
fn select2_waits_for_ready_future() {
    struct Pending;
    impl Future for Pending {
        type Output = usize;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            std::task::Poll::Pending
        }
    }

    let result = block_on(async { select2(Pending, async { 11usize }).await });
    match result {
        Either::Second(value) => assert_eq!(value, 11),
        Either::First(_) => panic!("second future should resolve"),
    }
}

#[test]
fn catch_unwind_yields_ok_on_success() {
    let result = block_on(async { catch_unwind(async { 7_u32 }).await });
    assert_eq!(result.expect("future should succeed"), 7);
}

#[test]
fn catch_unwind_traps_panics() {
    let result = block_on(async {
        catch_unwind(async {
            panic!("boom");
        })
        .await
    });
    assert!(result.is_err(), "panic should be captured");
}

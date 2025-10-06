use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use runtime::sleep;
use runtime::sync::broadcast;
use runtime::sync::mpsc;
use runtime::sync::oneshot;
use runtime::sync::semaphore::Semaphore;
use runtime::sync::{CancellationToken, Mutex};

#[test]
fn semaphore_allows_multiple_acquirers() {
    runtime::block_on(async {
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_clone = Arc::clone(&acquired);
        let sem_clone = Arc::clone(&semaphore);
        let handle = runtime::spawn(async move {
            let _second = sem_clone.acquire_owned().await.unwrap();
            acquired_clone.store(true, Ordering::SeqCst);
        });
        sleep(Duration::from_millis(10)).await;
        assert!(!acquired.load(Ordering::SeqCst));
        drop(permit);
        handle.await.unwrap();
        assert!(acquired.load(Ordering::SeqCst));
    });
}

#[test]
fn oneshot_channel_transfers_value() {
    runtime::block_on(async {
        let (tx, rx) = oneshot::channel();
        let sender = runtime::spawn(async move {
            tx.send(42).unwrap();
        });
        let value = rx.await.unwrap();
        sender.await.unwrap();
        assert_eq!(value, 42);

        let (tx, rx) = oneshot::channel::<u8>();
        drop(tx);
        assert!(matches!(rx.await, Err(oneshot::Canceled)));
    });
}

#[test]
fn mpsc_bounded_backpressure() {
    runtime::block_on(async {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(1).await.unwrap();
        let sender = runtime::spawn(async move {
            tx.send(2).await.unwrap();
        });
        sleep(Duration::from_millis(10)).await;
        assert_eq!(rx.try_recv().unwrap(), 1);
        assert_eq!(rx.recv().await.unwrap(), 2);
        sender.await.unwrap();
    });
}

#[test]
fn mpsc_unbounded_send_receive() {
    runtime::block_on(async {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send("hello").unwrap();
        tx.send("world").unwrap();
        assert_eq!(rx.recv().await.unwrap(), "hello");
        assert_eq!(rx.recv().await.unwrap(), "world");
    });
}

#[test]
fn broadcast_delivers_to_subscribers() {
    runtime::block_on(async {
        let (tx, mut rx) = broadcast::channel(1);
        tx.send(1usize).unwrap();
        tx.send(2).unwrap();
        assert!(matches!(
            rx.recv().await,
            Err(broadcast::error::RecvError::Lagged(1))
        ));
        assert_eq!(rx.recv().await.unwrap(), 2);

        let mut other = tx.subscribe();
        tx.send(3).unwrap();
        assert_eq!(other.recv().await.unwrap(), 3);
    });
}

#[test]
fn cancellation_token_notifies_waiters() {
    runtime::block_on(async {
        let token = CancellationToken::new();
        let fired = Arc::new(AtomicBool::new(false));
        let clone = fired.clone();
        let waiter = {
            let token = token.clone();
            runtime::spawn(async move {
                token.cancelled().await;
                clone.store(true, Ordering::SeqCst);
            })
        };

        sleep(Duration::from_millis(10)).await;
        assert!(!fired.load(Ordering::SeqCst));

        token.cancel();
        waiter.await.unwrap();
        assert!(fired.load(Ordering::SeqCst));
    });
}

#[test]
fn cancellation_token_wakes_multiple_waiters_and_is_idempotent() {
    runtime::block_on(async {
        let token = CancellationToken::new();
        let first = Arc::new(AtomicBool::new(false));
        let second = Arc::new(AtomicBool::new(false));

        let h1 = {
            let token = token.clone();
            let flag = first.clone();
            runtime::spawn(async move {
                token.cancelled().await;
                flag.store(true, Ordering::SeqCst);
            })
        };
        let h2 = {
            let token = token.clone();
            let flag = second.clone();
            runtime::spawn(async move {
                token.cancelled().await;
                flag.store(true, Ordering::SeqCst);
            })
        };

        sleep(Duration::from_millis(5)).await;
        token.cancel();
        token.cancel();

        h1.await.unwrap();
        h2.await.unwrap();
        assert!(first.load(Ordering::SeqCst));
        assert!(second.load(Ordering::SeqCst));

        // Additional listeners should observe an already-cancelled token immediately.
        token.cancelled().await;
    });
}

#[test]
fn mutex_provides_async_exclusion() {
    runtime::block_on(async {
        let mutex = Arc::new(Mutex::new(0_u32));
        let mut guard = mutex.lock().await;
        assert!(mutex.try_lock().is_none());

        let entered = Arc::new(AtomicBool::new(false));
        let entered_clone = entered.clone();
        let mutex_clone = mutex.clone();
        let waiter = runtime::spawn(async move {
            let mut guard = mutex_clone.lock().await;
            entered_clone.store(true, Ordering::SeqCst);
            *guard += 1;
        });

        sleep(Duration::from_millis(10)).await;
        assert!(!entered.load(Ordering::SeqCst));

        *guard += 1;
        drop(guard);

        waiter.await.unwrap();
        assert!(entered.load(Ordering::SeqCst));

        let guard = mutex.try_lock().expect("lock available after release");
        assert_eq!(*guard, 2);
    });
}

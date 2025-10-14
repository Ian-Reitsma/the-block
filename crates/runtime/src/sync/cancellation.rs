use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use foundation_async::task::AtomicWaker;

#[derive(Debug)]
struct Waiter {
    waker: AtomicWaker,
}

impl Waiter {
    fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
        }
    }

    fn register(&self, cx: &Context<'_>) {
        self.waker.register(cx.waker());
    }

    fn wake(&self) {
        self.waker.wake();
    }
}

#[derive(Debug)]
struct Inner {
    cancelled: AtomicBool,
    waiters: Mutex<Vec<Arc<Waiter>>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            waiters: Mutex::new(Vec::new()),
        }
    }

    fn wake_all(&self) {
        let waiters = self
            .waiters
            .lock()
            .expect("cancellation waiters poisoned")
            .drain(..)
            .collect::<Vec<_>>();
        for waiter in waiters {
            waiter.wake();
        }
    }

    fn push_waiter(&self, waiter: Arc<Waiter>) {
        self.waiters
            .lock()
            .expect("cancellation waiters poisoned")
            .push(waiter);
    }

    fn remove_waiter(&self, waiter: &Arc<Waiter>) {
        let mut waiters = self.waiters.lock().expect("cancellation waiters poisoned");
        if let Some(pos) = waiters.iter().position(|w| Arc::ptr_eq(w, waiter)) {
            waiters.remove(pos);
        }
    }
}

/// A cooperative cancellation primitive used to coordinate shutdown across async tasks.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<Inner>,
}

impl CancellationToken {
    /// Creates a new [`CancellationToken`].
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner::new()),
        }
    }

    /// Signals cancellation and wakes any pending waiters.
    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::SeqCst) {
            self.inner.wake_all();
        }
    }

    /// Returns `true` if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Returns a future that resolves once the token is cancelled.
    pub fn cancelled(&self) -> Cancelled<'_> {
        Cancelled {
            token: self,
            waiter: None,
        }
    }
}

/// Future that resolves when the associated [`CancellationToken`] is cancelled.
pub struct Cancelled<'a> {
    token: &'a CancellationToken,
    waiter: Option<Arc<Waiter>>,
}

impl<'a> Future for Cancelled<'a> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.token.is_cancelled() {
            return Poll::Ready(());
        }

        let waiter = this.waiter.get_or_insert_with(|| {
            let waiter = Arc::new(Waiter::new());
            this.token.inner.push_waiter(waiter.clone());
            waiter
        });
        waiter.register(cx);

        if this.token.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl<'a> Drop for Cancelled<'a> {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            self.token.inner.remove_waiter(&waiter);
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

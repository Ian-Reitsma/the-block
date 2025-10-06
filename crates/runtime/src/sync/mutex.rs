use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context, Poll};

use futures::task::AtomicWaker;

#[derive(Debug)]
struct Waiter {
    waker: AtomicWaker,
    ready: AtomicBool,
}

impl Waiter {
    fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            ready: AtomicBool::new(false),
        }
    }

    fn register(&self, cx: &Context<'_>) {
        self.waker.register(cx.waker());
        if self.ready.load(Ordering::SeqCst) {
            self.waker.wake();
        }
    }

    fn wake(&self) {
        if !self.ready.swap(true, Ordering::SeqCst) {
            self.waker.wake();
        }
    }
}

#[derive(Debug)]
struct Inner<T> {
    value: Option<T>,
    locked: bool,
    waiters: VecDeque<Arc<Waiter>>,
}

impl<T> Inner<T> {
    fn new(value: T) -> Self {
        Self {
            value: Some(value),
            locked: false,
            waiters: VecDeque::new(),
        }
    }
}

/// An asynchronous mutex backed by the runtime primitives.
#[derive(Debug)]
pub struct Mutex<T> {
    inner: StdMutex<Inner<T>>,
}

impl<T> Mutex<T> {
    /// Creates a new mutex protecting the provided value.
    pub fn new(value: T) -> Self {
        Self {
            inner: StdMutex::new(Inner::new(value)),
        }
    }

    /// Returns a mutable reference to the contained value.
    pub fn get_mut(&mut self) -> &mut T {
        let inner = self.inner.get_mut().expect("mutex poisoned");
        inner.value.as_mut().expect("mutex value missing")
    }

    /// Consumes the mutex, returning the underlying value.
    pub fn into_inner(self) -> T {
        self.inner
            .into_inner()
            .expect("mutex poisoned")
            .value
            .expect("mutex value missing")
    }

    /// Attempts to acquire the mutex without waiting.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        let mut state = self.inner.lock().expect("mutex poisoned");
        if state.locked {
            return None;
        }
        state.locked = true;
        let value = state.value.take().expect("mutex value missing");
        Some(MutexGuard {
            mutex: self,
            value: Some(value),
        })
    }

    fn enqueue_waiter(&self, waiter: Arc<Waiter>) {
        let mut state = self.inner.lock().expect("mutex poisoned");
        state.waiters.push_back(waiter);
    }

    fn cancel_waiter(&self, waiter: &Arc<Waiter>) {
        let mut state = self.inner.lock().expect("mutex poisoned");
        if let Some(pos) = state
            .waiters
            .iter()
            .position(|slot| Arc::ptr_eq(slot, waiter))
        {
            state.waiters.remove(pos);
        }
    }

    fn acquire(&self) -> Option<T> {
        let mut state = self.inner.lock().expect("mutex poisoned");
        if state.locked {
            return None;
        }
        state.locked = true;
        Some(state.value.take().expect("mutex value missing"))
    }

    fn release(&self, mut value: Option<T>) {
        let mut state = self.inner.lock().expect("mutex poisoned");
        assert!(state.locked, "mutex released while unlocked");
        assert!(state.value.is_none(), "mutex double release");
        state.value = value.take();
        state.locked = false;
        if let Some(waiter) = state.waiters.pop_front() {
            drop(state);
            waiter.wake();
        }
    }

    /// Acquires the mutex, returning a future that resolves once the lock is held.
    pub fn lock(&self) -> Lock<'_, T> {
        Lock {
            mutex: self,
            waiter: None,
        }
    }
}

/// Future returned by [`Mutex::lock`].
pub struct Lock<'a, T> {
    mutex: &'a Mutex<T>,
    waiter: Option<Arc<Waiter>>,
}

impl<'a, T> Future for Lock<'a, T> {
    type Output = MutexGuard<'a, T>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(value) = self.mutex.acquire() {
            self.waiter = None;
            return Poll::Ready(MutexGuard {
                mutex: self.mutex,
                value: Some(value),
            });
        }

        match &self.waiter {
            Some(waiter) => waiter.register(cx),
            None => {
                let waiter = Arc::new(Waiter::new());
                waiter.register(cx);
                self.mutex.enqueue_waiter(waiter.clone());
                self.waiter = Some(waiter);
            }
        }
        Poll::Pending
    }
}

impl<'a, T> Drop for Lock<'a, T> {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            self.mutex.cancel_waiter(&waiter);
        }
    }
}

/// Guard returned from [`Mutex::lock`] and [`Mutex::try_lock`].
pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
    value: Option<T>,
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        let value = self.value.take();
        self.mutex.release(value);
    }
}

impl<'a, T> std::ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().expect("mutex guard missing value")
    }
}

impl<'a, T> std::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().expect("mutex guard missing value")
    }
}

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};

use foundation_async::task::AtomicWaker;

/// Error returned when a semaphore is closed and no further permits can be acquired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcquireError;

impl fmt::Display for AcquireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("semaphore closed")
    }
}

impl std::error::Error for AcquireError {}

/// Counting semaphore that integrates with the runtime executor.
#[derive(Debug)]
pub struct Semaphore {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    state: Mutex<State>,
    available: Condvar,
}

#[derive(Debug)]
struct State {
    permits: usize,
    closed: bool,
    waiters: VecDeque<Arc<Waiter>>,
}

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

    fn register(&self, waker: &Waker) {
        self.waker.register(waker);
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

impl Semaphore {
    /// Creates a new semaphore with the provided number of permits.
    pub fn new(permits: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    permits,
                    closed: false,
                    waiters: VecDeque::new(),
                }),
                available: Condvar::new(),
            }),
        }
    }

    /// Returns the number of permits currently available.
    pub fn available_permits(&self) -> usize {
        let state = self.inner.state.lock().expect("semaphore poisoned");
        state.permits
    }

    /// Adds permits back to the semaphore.
    pub fn add_permits(&self, permits: usize) {
        if permits == 0 {
            return;
        }
        let mut state = self.inner.state.lock().expect("semaphore poisoned");
        state.permits = state.permits.saturating_add(permits);
        for _ in 0..permits {
            if let Some(waiter) = state.waiters.pop_front() {
                waiter.wake();
            } else {
                break;
            }
        }
        drop(state);
        self.inner.available.notify_all();
    }

    /// Prevents additional permits from being acquired. Existing waiters receive an error.
    pub fn close(&self) {
        let mut state = self.inner.state.lock().expect("semaphore poisoned");
        if state.closed {
            return;
        }
        state.closed = true;
        let waiters = state.waiters.drain(..).collect::<Vec<_>>();
        drop(state);
        for waiter in waiters {
            waiter.wake();
        }
        self.inner.available.notify_all();
    }

    /// Returns a future that waits until a permit is available.
    pub fn acquire(&self) -> Acquire<'_> {
        Acquire {
            semaphore: &self.inner,
            waiter: None,
        }
    }

    /// Blocks the current thread until a permit becomes available.
    pub fn blocking_acquire(&self) -> Result<Permit<'_>, AcquireError> {
        let mut state = self.inner.state.lock().expect("semaphore poisoned");
        loop {
            if state.permits > 0 {
                state.permits -= 1;
                drop(state);
                return Ok(Permit {
                    semaphore: &self.inner,
                    released: false,
                });
            }
            if state.closed {
                drop(state);
                return Err(AcquireError);
            }
            state = self
                .inner
                .available
                .wait(state)
                .expect("semaphore poisoned");
        }
    }

    /// Returns a future that waits until a permit is available and owns the permit.
    pub fn acquire_owned(self: &Arc<Self>) -> AcquireOwned {
        AcquireOwned {
            semaphore: Arc::clone(&self.inner),
            waiter: None,
        }
    }

    fn poll_acquire(
        inner: &Arc<Inner>,
        waiter_slot: &mut Option<Arc<Waiter>>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), AcquireError>> {
        let mut state = inner.state.lock().expect("semaphore poisoned");
        if state.permits > 0 {
            state.permits -= 1;
            if let Some(waiter) = waiter_slot.take() {
                drop(state);
                drop(waiter);
            } else {
                drop(state);
            }
            return Poll::Ready(Ok(()));
        }
        if state.closed {
            drop(state);
            return Poll::Ready(Err(AcquireError));
        }

        if let Some(waiter) = waiter_slot {
            waiter.register(cx.waker());
        } else {
            let waiter = Arc::new(Waiter::new());
            waiter.register(cx.waker());
            state.waiters.push_back(waiter.clone());
            *waiter_slot = Some(waiter);
        }
        Poll::Pending
    }

    fn release(inner: &Arc<Inner>) {
        let mut state = inner.state.lock().expect("semaphore poisoned");
        state.permits = state.permits.saturating_add(1);
        if let Some(waiter) = state.waiters.pop_front() {
            waiter.wake();
        }
        drop(state);
        inner.available.notify_one();
    }

    fn cancel_waiter(inner: &Arc<Inner>, waiter: &Arc<Waiter>) {
        let mut state = inner.state.lock().expect("semaphore poisoned");
        if let Some(pos) = state.waiters.iter().position(|w| Arc::ptr_eq(w, waiter)) {
            state.waiters.remove(pos);
        }
    }
}

/// A permit acquired from a [`Semaphore`] reference.
pub struct Permit<'a> {
    semaphore: &'a Arc<Inner>,
    released: bool,
}

impl<'a> Permit<'a> {
    /// Releases the held permit back to the semaphore.
    pub fn release(mut self) {
        if !self.released {
            self.released = true;
            Semaphore::release(self.semaphore);
        }
    }
}

impl<'a> Drop for Permit<'a> {
    fn drop(&mut self) {
        if !self.released {
            self.released = true;
            Semaphore::release(self.semaphore);
        }
    }
}

/// An owned permit returned by [`Semaphore::acquire_owned`].
pub struct OwnedSemaphorePermit {
    semaphore: Arc<Inner>,
    released: bool,
}

impl OwnedSemaphorePermit {
    /// Releases the held permit back to the semaphore.
    pub fn release(mut self) {
        if !self.released {
            self.released = true;
            Semaphore::release(&self.semaphore);
        }
    }
}

impl Drop for OwnedSemaphorePermit {
    fn drop(&mut self) {
        if !self.released {
            self.released = true;
            Semaphore::release(&self.semaphore);
        }
    }
}

/// Future returned from [`Semaphore::acquire`].
pub struct Acquire<'a> {
    semaphore: &'a Arc<Inner>,
    waiter: Option<Arc<Waiter>>,
}

impl<'a> Future for Acquire<'a> {
    type Output = Result<Permit<'a>, AcquireError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        match Semaphore::poll_acquire(this.semaphore, &mut this.waiter, cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(Permit {
                semaphore: this.semaphore,
                released: false,
            })),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<'a> Drop for Acquire<'a> {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            Semaphore::cancel_waiter(self.semaphore, &waiter);
        }
    }
}

/// Future returned from [`Semaphore::acquire_owned`].
pub struct AcquireOwned {
    semaphore: Arc<Inner>,
    waiter: Option<Arc<Waiter>>,
}

impl Future for AcquireOwned {
    type Output = Result<OwnedSemaphorePermit, AcquireError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        match Semaphore::poll_acquire(&this.semaphore, &mut this.waiter, cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(OwnedSemaphorePermit {
                semaphore: Arc::clone(&this.semaphore),
                released: false,
            })),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for AcquireOwned {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            Semaphore::cancel_waiter(&self.semaphore, &waiter);
        }
    }
}

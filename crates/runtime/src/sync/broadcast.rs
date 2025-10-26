use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LockResult, Mutex};
use std::task::{Context, Poll};

use foundation_async::task::AtomicWaker;

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

trait LockResultExt<T> {
    fn recover(self) -> T;
}

impl<T> LockResultExt<T> for LockResult<T> {
    fn recover(self) -> T {
        match self {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

struct Message<T> {
    sequence: u64,
    value: Arc<T>,
}

struct Inner<T> {
    state: Mutex<State<T>>,
    capacity: usize,
    subscriber_count: AtomicUsize,
    sender_count: AtomicUsize,
    closed: AtomicBool,
}

struct State<T> {
    buffer: VecDeque<Message<T>>,
    next_sequence: u64,
    waiters: VecDeque<Arc<Waiter>>,
}

impl<T> Inner<T> {
    fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                buffer: VecDeque::new(),
                next_sequence: 0,
                waiters: VecDeque::new(),
            }),
            capacity,
            subscriber_count: AtomicUsize::new(1),
            sender_count: AtomicUsize::new(1),
            closed: AtomicBool::new(false),
        })
    }

    fn wake_waiters(&self, mut state: std::sync::MutexGuard<'_, State<T>>) {
        let waiters = state.waiters.drain(..).collect::<Vec<_>>();
        drop(state);
        for waiter in waiters {
            waiter.wake();
        }
    }

    fn cancel_waiter(&self, waiter: &Arc<Waiter>) {
        let mut state = self.state.lock().recover();
        if let Some(pos) = state.waiters.iter().position(|w| Arc::ptr_eq(w, waiter)) {
            state.waiters.remove(pos);
        }
    }
}

/// Error returned when sending to a broadcast channel fails.
pub struct SendError<T> {
    value: Option<T>,
}

impl<T> SendError<T> {
    fn new(value: T) -> Self {
        Self { value: Some(value) }
    }

    /// Extracts the value that failed to send.
    pub fn into_inner(mut self) -> T {
        self.value
            .take()
            .unwrap_or_else(|| panic!("send error value already taken"))
    }
}

impl<T> fmt::Debug for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendError").finish_non_exhaustive()
    }
}

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no subscribers")
    }
}

impl<T> std::error::Error for SendError<T> {}

/// Error returned when receiving from a broadcast channel fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvError {
    Closed,
    Lagged(u64),
}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecvError::Closed => f.write_str("channel closed"),
            RecvError::Lagged(n) => write!(f, "dropped {n} messages"),
        }
    }
}

impl std::error::Error for RecvError {}

/// Creates a broadcast channel.
pub fn channel<T: Clone>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Inner::new(capacity.max(1));
    (
        Sender {
            inner: Arc::clone(&inner),
        },
        Receiver {
            inner,
            next_sequence: 0,
            waiter: None,
        },
    )
}

/// Sender half of a broadcast channel.
#[derive(Clone)]
pub struct Sender<T: Clone> {
    inner: Arc<Inner<T>>,
}

impl<T: Clone> Sender<T> {
    /// Sends a new value to all subscribers.
    pub fn send(&self, value: T) -> Result<usize, SendError<T>> {
        let subscribers = self.inner.subscriber_count.load(Ordering::SeqCst);
        if subscribers == 0 {
            return Err(SendError::new(value));
        }
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(SendError::new(value));
        }

        let mut state = self.state();
        let message = Arc::new(value);
        let sequence = state.next_sequence;
        state.next_sequence += 1;
        state.buffer.push_back(Message {
            sequence,
            value: message,
        });
        if state.buffer.len() > self.inner.capacity {
            state.buffer.pop_front();
        }
        self.inner.wake_waiters(state);
        Ok(subscribers)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State<T>> {
        self.inner.state.lock().recover()
    }

    /// Subscribes to the channel, receiving only new messages.
    pub fn subscribe(&self) -> Receiver<T> {
        self.inner.subscriber_count.fetch_add(1, Ordering::SeqCst);
        let state = self.state();
        let next = state.next_sequence;
        drop(state);
        Receiver {
            inner: Arc::clone(&self.inner),
            next_sequence: next,
            waiter: None,
        }
    }

    /// Closes the channel. Receivers will observe [`RecvError::Closed`].
    pub fn close(&self) {
        if self.inner.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let state = self.state();
        self.inner.wake_waiters(state);
    }
}

impl<T: Clone> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.inner.sender_count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.close();
        }
    }
}

/// Receiver half of a broadcast channel.
pub struct Receiver<T: Clone> {
    inner: Arc<Inner<T>>,
    next_sequence: u64,
    waiter: Option<Arc<Waiter>>,
}

impl<T: Clone> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.inner.subscriber_count.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: Arc::clone(&self.inner),
            next_sequence: self.next_sequence,
            waiter: None,
        }
    }
}

impl<T: Clone> Receiver<T> {
    /// Waits for the next broadcast value.
    pub fn recv(&mut self) -> Recv<'_, T> {
        Recv { receiver: self }
    }

    fn poll_recv_internal(&mut self, cx: &mut Context<'_>) -> Poll<Result<T, RecvError>> {
        let mut state = self.inner.state.lock().recover();
        if let Some(front) = state.buffer.front() {
            if front.sequence > self.next_sequence {
                let missed = front.sequence - self.next_sequence;
                self.next_sequence = front.sequence;
                self.waiter = None;
                drop(state);
                return Poll::Ready(Err(RecvError::Lagged(missed)));
            }
            let front_seq = front.sequence;
            if let Some(msg) = state.buffer.get((self.next_sequence - front_seq) as usize) {
                let value = (*msg.value).clone();
                self.next_sequence += 1;
                self.waiter = None;
                drop(state);
                return Poll::Ready(Ok(value));
            }
        }
        if self.inner.closed.load(Ordering::SeqCst) {
            self.waiter = None;
            drop(state);
            return Poll::Ready(Err(RecvError::Closed));
        }
        if let Some(waiter) = &self.waiter {
            waiter.register(cx);
        } else {
            let waiter = Arc::new(Waiter::new());
            waiter.register(cx);
            state.waiters.push_back(waiter.clone());
            self.waiter = Some(waiter);
        }
        drop(state);
        Poll::Pending
    }
}

impl<T: Clone> Drop for Receiver<T> {
    fn drop(&mut self) {
        if self.inner.subscriber_count.fetch_sub(1, Ordering::SeqCst) == 1 {
            let mut state = self.inner.state.lock().recover();
            let waiters = state.waiters.drain(..).collect::<Vec<_>>();
            drop(state);
            for waiter in waiters {
                waiter.wake();
            }
        }
        if let Some(waiter) = self.waiter.take() {
            let mut state = self.inner.state.lock().recover();
            if let Some(pos) = state.waiters.iter().position(|w| Arc::ptr_eq(w, &waiter)) {
                state.waiters.remove(pos);
            }
        }
    }
}

/// Future returned from [`Receiver::recv`].
pub struct Recv<'a, T: Clone> {
    receiver: &'a mut Receiver<T>,
}

impl<T: Clone> Future for Recv<'_, T> {
    type Output = Result<T, RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let receiver = &mut self.receiver;
        receiver.poll_recv_internal(cx)
    }
}

impl<T: Clone> Drop for Recv<'_, T> {
    fn drop(&mut self) {
        if let Some(waiter) = self.receiver.waiter.take() {
            self.receiver.inner.cancel_waiter(&waiter);
        }
    }
}

pub mod error {
    pub use super::{RecvError, SendError};
}

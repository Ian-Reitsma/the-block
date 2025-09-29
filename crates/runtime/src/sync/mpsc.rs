use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};

use futures::stream::Stream;
use futures::task::AtomicWaker;
use pin_project::pin_project;
use pin_project::pinned_drop;

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

#[derive(Debug, Clone, Copy)]
enum Capacity {
    Bounded(usize),
    Unbounded,
}

#[derive(Debug)]
struct Inner<T> {
    state: Mutex<State<T>>,
    capacity: Capacity,
    available: Condvar,
    sender_count: AtomicUsize,
}

#[derive(Debug)]
struct State<T> {
    queue: VecDeque<T>,
    receiver_alive: bool,
    sender_waiters: VecDeque<Arc<Waiter>>,
    receiver_waiters: VecDeque<Arc<Waiter>>,
}

impl<T> Inner<T> {
    fn new(capacity: Capacity) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                receiver_alive: true,
                sender_waiters: VecDeque::new(),
                receiver_waiters: VecDeque::new(),
            }),
            capacity,
            available: Condvar::new(),
            sender_count: AtomicUsize::new(1),
        })
    }

    fn wake_next_sender(&self, mut state: std::sync::MutexGuard<'_, State<T>>) {
        if let Some(waiter) = state.sender_waiters.pop_front() {
            drop(state);
            waiter.wake();
        } else {
            drop(state);
            self.available.notify_one();
        }
    }

    fn wake_next_receiver(&self, mut state: std::sync::MutexGuard<'_, State<T>>) {
        if let Some(waiter) = state.receiver_waiters.pop_front() {
            drop(state);
            waiter.wake();
        } else {
            drop(state);
        }
    }

    fn cancel_sender_waiter(&self, waiter: &Arc<Waiter>) {
        let mut state = self.state.lock().expect("mpsc poisoned");
        if let Some(pos) = state
            .sender_waiters
            .iter()
            .position(|w| Arc::ptr_eq(w, waiter))
        {
            state.sender_waiters.remove(pos);
        }
    }

    fn cancel_receiver_waiter(&self, waiter: &Arc<Waiter>) {
        let mut state = self.state.lock().expect("mpsc poisoned");
        if let Some(pos) = state
            .receiver_waiters
            .iter()
            .position(|w| Arc::ptr_eq(w, waiter))
        {
            state.receiver_waiters.remove(pos);
        }
    }

    fn poll_send(
        &self,
        value: &mut Option<T>,
        waiter_slot: &mut Option<Arc<Waiter>>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), SendError<T>>> {
        let mut state = self.state.lock().expect("mpsc poisoned");
        if !state.receiver_alive {
            let value = value.take().expect("value");
            drop(state);
            return Poll::Ready(Err(SendError::new(value)));
        }
        let can_push = match self.capacity {
            Capacity::Unbounded => true,
            Capacity::Bounded(cap) => state.queue.len() < cap,
        };
        if can_push {
            let v = value.take().expect("value");
            state.queue.push_back(v);
            self.wake_next_receiver(state);
            return Poll::Ready(Ok(()));
        }
        if let Some(waiter) = waiter_slot {
            waiter.register(cx);
        } else {
            let waiter = Arc::new(Waiter::new());
            waiter.register(cx);
            state.sender_waiters.push_back(waiter.clone());
            *waiter_slot = Some(waiter);
        }
        drop(state);
        Poll::Pending
    }
}

/// Error returned when sending on a closed channel.
pub struct SendError<T> {
    value: Option<T>,
}

impl<T> SendError<T> {
    fn new(value: T) -> Self {
        Self { value: Some(value) }
    }

    /// Extracts the value that failed to send.
    pub fn into_inner(mut self) -> T {
        self.value.take().expect("value already taken")
    }
}

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("channel closed")
    }
}

impl<T> fmt::Debug for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendError").finish_non_exhaustive()
    }
}

impl<T> std::error::Error for SendError<T> {}

/// Creates a bounded channel.
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Inner::new(Capacity::Bounded(capacity));
    (
        Sender {
            inner: Arc::clone(&inner),
        },
        Receiver {
            inner,
            closed: false,
        },
    )
}

/// Creates an unbounded channel.
pub fn unbounded_channel<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let inner = Inner::new(Capacity::Unbounded);
    (
        UnboundedSender {
            inner: Arc::clone(&inner),
        },
        UnboundedReceiver {
            inner: Receiver {
                inner,
                closed: false,
            },
        },
    )
}

/// Sends values into the channel, waiting asynchronously when the channel is full.
#[derive(Debug)]
pub struct Sender<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.inner.sender_count.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Sender<T> {
    /// Returns a future that sends a value into the channel.
    pub fn send(&self, value: T) -> Send<T> {
        Send {
            inner: Arc::clone(&self.inner),
            value: Some(value),
            waiter: None,
        }
    }

    /// Blocks the current thread until the value is sent.
    pub fn blocking_send(&self, value: T) -> Result<(), SendError<T>> {
        let mut value = Some(value);
        let mut state = self.inner.state.lock().expect("mpsc poisoned");
        loop {
            if !state.receiver_alive {
                let err = SendError::new(value.take().expect("value"));
                drop(state);
                return Err(err);
            }
            let can_push = match self.inner.capacity {
                Capacity::Unbounded => true,
                Capacity::Bounded(cap) => state.queue.len() < cap,
            };
            if can_push {
                let v = value.take().expect("value");
                state.queue.push_back(v);
                self.inner.wake_next_receiver(state);
                return Ok(());
            }
            state = self.inner.available.wait(state).expect("mpsc poisoned");
        }
    }

    fn last_sender_dropped(&self) {
        let mut state = self.inner.state.lock().expect("mpsc poisoned");
        let waiters = state.receiver_waiters.drain(..).collect::<Vec<_>>();
        drop(state);
        for waiter in waiters {
            waiter.wake();
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.inner.sender_count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.last_sender_dropped();
        }
    }
}

#[pin_project(PinnedDrop)]
pub struct Send<T> {
    inner: Arc<Inner<T>>,
    value: Option<T>,
    waiter: Option<Arc<Waiter>>,
}

impl<T> Future for Send<T> {
    type Output = Result<(), SendError<T>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.inner.poll_send(this.value, this.waiter, cx)
    }
}

#[pinned_drop]
impl<T> PinnedDrop for Send<T> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        if let Some(waiter) = this.waiter.take() {
            this.inner.cancel_sender_waiter(&waiter);
        }
    }
}

/// Receives values from the channel.
#[derive(Debug)]
pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
    closed: bool,
}

impl<T> Receiver<T> {
    /// Receives the next value from the channel.
    pub fn recv(&mut self) -> Recv<'_, T> {
        Recv {
            receiver: self,
            waiter: None,
        }
    }

    fn poll_recv_internal(
        &self,
        waiter_slot: &mut Option<Arc<Waiter>>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<T>> {
        let mut state = self.inner.state.lock().expect("mpsc poisoned");
        if let Some(value) = state.queue.pop_front() {
            self.inner.wake_next_sender(state);
            return Poll::Ready(Some(value));
        }
        if !state.receiver_alive {
            drop(state);
            return Poll::Ready(None);
        }
        if self.inner.sender_count.load(Ordering::SeqCst) == 0 {
            drop(state);
            return Poll::Ready(None);
        }
        if let Some(waiter) = waiter_slot {
            waiter.register(cx);
        } else {
            let waiter = Arc::new(Waiter::new());
            waiter.register(cx);
            state.receiver_waiters.push_back(waiter.clone());
            *waiter_slot = Some(waiter);
        }
        drop(state);
        Poll::Pending
    }

    /// Attempts to receive a value without waiting.
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        let mut state = self.inner.state.lock().expect("mpsc poisoned");
        if let Some(value) = state.queue.pop_front() {
            self.inner.wake_next_sender(state);
            Ok(value)
        } else if self.inner.sender_count.load(Ordering::SeqCst) == 0 {
            drop(state);
            Err(TryRecvError::Closed)
        } else {
            drop(state);
            Err(TryRecvError::Empty)
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let mut state = self.inner.state.lock().expect("mpsc poisoned");
        if !state.receiver_alive {
            drop(state);
            return;
        }
        state.receiver_alive = false;
        let sender_waiters = state.sender_waiters.drain(..).collect::<Vec<_>>();
        drop(state);
        for waiter in sender_waiters {
            waiter.wake();
        }
        self.inner.available.notify_all();
    }
}

/// Future returned from [`Receiver::recv`].
pub struct Recv<'a, T> {
    receiver: &'a Receiver<T>,
    waiter: Option<Arc<Waiter>>,
}

impl<'a, T> Future for Recv<'a, T> {
    type Output = Option<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        this.receiver.poll_recv_internal(&mut this.waiter, cx)
    }
}

impl<'a, T> Drop for Recv<'a, T> {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            self.receiver.inner.cancel_receiver_waiter(&waiter);
        }
    }
}

/// Error returned by [`Receiver::try_recv`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    Empty,
    Closed,
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TryRecvError::Empty => f.write_str("channel empty"),
            TryRecvError::Closed => f.write_str("channel closed"),
        }
    }
}

impl std::error::Error for TryRecvError {}

/// Sender half for unbounded channels.
#[derive(Debug)]
pub struct UnboundedSender<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Clone for UnboundedSender<T> {
    fn clone(&self) -> Self {
        self.inner.sender_count.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> UnboundedSender<T> {
    /// Sends a value without waiting.
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        let mut state = self.inner.state.lock().expect("mpsc poisoned");
        if !state.receiver_alive {
            return Err(SendError::new(value));
        }
        state.queue.push_back(value);
        self.inner.wake_next_receiver(state);
        Ok(())
    }
}

impl<T> Drop for UnboundedSender<T> {
    fn drop(&mut self) {
        if self.inner.sender_count.fetch_sub(1, Ordering::SeqCst) == 1 {
            let mut state = self.inner.state.lock().expect("mpsc poisoned");
            let waiters = state.receiver_waiters.drain(..).collect::<Vec<_>>();
            drop(state);
            for waiter in waiters {
                waiter.wake();
            }
        }
    }
}

/// Receiver half for unbounded channels.
#[derive(Debug)]
pub struct UnboundedReceiver<T> {
    inner: Receiver<T>,
}

impl<T> UnboundedReceiver<T> {
    /// Receives the next value from the channel.
    pub fn recv(&mut self) -> Recv<'_, T> {
        self.inner.recv()
    }

    /// Attempts to receive a value without waiting.
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        self.inner.try_recv()
    }

    /// Converts this receiver into a stream.
    pub fn into_stream(self) -> ReceiverStream<T> {
        ReceiverStream::new(self.inner)
    }
}

/// Stream wrapper around a [`Receiver`].
#[derive(Debug)]
pub struct ReceiverStream<T> {
    receiver: Receiver<T>,
    waiter: Option<Arc<Waiter>>,
}

impl<T> ReceiverStream<T> {
    pub fn new(receiver: Receiver<T>) -> Self {
        Self {
            receiver,
            waiter: None,
        }
    }
}

impl<T> Stream for ReceiverStream<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.receiver.poll_recv_internal(&mut this.waiter, cx)
    }
}

impl<T> Drop for ReceiverStream<T> {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            self.receiver.inner.cancel_receiver_waiter(&waiter);
        }
    }
}

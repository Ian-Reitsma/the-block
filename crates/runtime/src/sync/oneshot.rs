use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::task::AtomicWaker;

/// Error returned when the sender half of a oneshot channel is dropped before sending a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Canceled;

impl fmt::Display for Canceled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("oneshot channel canceled")
    }
}

impl std::error::Error for Canceled {}

/// Creates a new oneshot channel.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Inner {
        state: Mutex::new(State::Pending),
        receiver_waker: AtomicWaker::new(),
        sender_alive: AtomicBool::new(true),
        receiver_alive: AtomicBool::new(true),
    });
    (
        Sender {
            inner: Arc::clone(&inner),
        },
        Receiver { inner },
    )
}

#[derive(Debug)]
struct Inner<T> {
    state: Mutex<State<T>>,
    receiver_waker: AtomicWaker,
    sender_alive: AtomicBool,
    receiver_alive: AtomicBool,
}

#[derive(Debug)]
enum State<T> {
    Pending,
    Value(T),
    Consumed,
    Closed,
}

/// Sends a single value to the receiving half of the channel.
pub struct Sender<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Sender<T> {
    /// Sends a value along the channel.
    pub fn send(self, value: T) -> Result<(), T> {
        self.send_inner(value)
    }

    fn send_inner(self, value: T) -> Result<(), T> {
        let mut state = self.inner.state.lock().expect("oneshot poisoned");
        if matches!(*state, State::Consumed | State::Closed) {
            return Err(value);
        }
        if self.inner.receiver_alive.load(Ordering::SeqCst) {
            *state = State::Value(value);
            drop(state);
            self.inner.receiver_waker.wake();
            Ok(())
        } else {
            Err(value)
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.inner.sender_alive.swap(false, Ordering::SeqCst) {
            let mut state = self.inner.state.lock().expect("oneshot poisoned");
            if matches!(*state, State::Pending) {
                *state = State::Closed;
                drop(state);
                self.inner.receiver_waker.wake();
            }
        }
    }
}

/// Future that resolves when a value is sent on the channel.
pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Receiver<T> {
    /// Attempts to cancel the channel. If the value has not yet been sent, the sender
    /// will receive an error when attempting to send.
    pub fn close(&self) {
        if self.inner.receiver_alive.swap(false, Ordering::SeqCst) {
            let mut state = self.inner.state.lock().expect("oneshot poisoned");
            if matches!(*state, State::Pending) {
                *state = State::Closed;
                drop(state);
            }
        }
    }
}

impl<T> Future for Receiver<T> {
    type Output = Result<T, Canceled>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = &self.as_ref().get_ref().inner;
        let mut state = inner.state.lock().expect("oneshot poisoned");
        match std::mem::replace(&mut *state, State::Consumed) {
            State::Value(v) => {
                *state = State::Consumed;
                drop(state);
                Poll::Ready(Ok(v))
            }
            State::Consumed => {
                *state = State::Consumed;
                Poll::Pending
            }
            State::Closed => {
                *state = State::Closed;
                Poll::Ready(Err(Canceled))
            }
            State::Pending => {
                *state = State::Pending;
                inner.receiver_waker.register(cx.waker());
                if !inner.sender_alive.load(Ordering::SeqCst) {
                    *state = State::Closed;
                    drop(state);
                    return Poll::Ready(Err(Canceled));
                }
                Poll::Pending
            }
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if self.inner.receiver_alive.swap(false, Ordering::SeqCst) {
            let mut state = self.inner.state.lock().expect("oneshot poisoned");
            if matches!(*state, State::Pending) {
                *state = State::Closed;
                drop(state);
            }
        }
    }
}

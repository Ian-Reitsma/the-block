use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::task::Waker;

/// Minimal `AtomicWaker` replacement used by async primitives throughout the workspace.
#[derive(Debug)]
pub struct AtomicWaker {
    waker: Mutex<Option<Waker>>,
    pending: AtomicBool,
}

impl AtomicWaker {
    pub fn new() -> Self {
        Self {
            waker: Mutex::new(None),
            pending: AtomicBool::new(false),
        }
    }

    /// Registers the provided waker, replacing any previously stored handle.
    ///
    /// If a wakeup arrived before registration, the newly stored waker is
    /// notified immediately so the caller does not miss the signal.
    pub fn register(&self, waker: &Waker) {
        let mut slot = self.waker.lock().unwrap_or_else(|err| err.into_inner());
        let should_replace = slot
            .as_ref()
            .is_none_or(|current| !current.will_wake(waker));
        if should_replace {
            *slot = Some(waker.clone());
        }

        let notify = if self.pending.swap(false, Ordering::SeqCst) {
            slot.as_ref().cloned()
        } else {
            None
        };
        drop(slot);
        if let Some(waker) = notify {
            waker.wake_by_ref();
        }
    }

    /// Wakes the stored waker, if any. If no waker is registered yet, the wake
    /// is remembered and delivered once a waker is installed via [`register`].
    pub fn wake(&self) {
        if let Some(waker) = self
            .waker
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
        {
            waker.wake();
        } else {
            self.pending.store(true, Ordering::SeqCst);
        }
    }

    /// Wakes the stored waker by reference, if any. If no waker is available
    /// yet, the wakeup is deferred to the next call to [`register`].
    pub fn wake_by_ref(&self) {
        if let Some(waker) = self
            .waker
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
        {
            waker.wake_by_ref();
        } else {
            self.pending.store(true, Ordering::SeqCst);
        }
    }
}

impl Default for AtomicWaker {
    fn default() -> Self {
        Self::new()
    }
}

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread;

struct ThreadNotify {
    thread: thread::Thread,
    notified: AtomicBool,
}

impl ThreadNotify {
    fn new() -> Self {
        Self {
            thread: thread::current(),
            notified: AtomicBool::new(false),
        }
    }

    fn wait(&self) {
        while !self.notified.swap(false, Ordering::SeqCst) {
            thread::park();
        }
    }
}

impl Wake for ThreadNotify {
    fn wake(self: Arc<Self>) {
        if !self.notified.swap(true, Ordering::SeqCst) {
            self.thread.unpark();
        }
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if !self.notified.swap(true, Ordering::SeqCst) {
            self.thread.unpark();
        }
    }
}

fn build_waker(notify: &Arc<ThreadNotify>) -> Waker {
    Waker::from(Arc::clone(notify))
}

/// Runs the provided future to completion on the current thread.
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    let mut future = Box::pin(future);
    let notify = Arc::new(ThreadNotify::new());

    loop {
        let waker = build_waker(&notify);
        let mut cx = Context::from_waker(&waker);
        match Future::poll(Pin::as_mut(&mut future), &mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => notify.wait(),
        }
    }
}

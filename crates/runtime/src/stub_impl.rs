use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor;
use pin_project_lite::pin_project;

type PendingCounter = Arc<AtomicI64>;

const SPAWN_LATENCY_METRIC: &str = "runtime_spawn_latency_seconds";
const PENDING_TASKS_METRIC: &str = "runtime_pending_tasks";

pub(crate) struct StubRuntime {
    pending: PendingCounter,
}

pub(crate) fn runtime() -> Arc<StubRuntime> {
    Arc::new(StubRuntime {
        pending: Arc::new(AtomicI64::new(0)),
    })
}

impl StubRuntime {
    pub(crate) fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        executor::block_on(future)
    }

    pub(crate) fn spawn<F, T>(&self, future: F) -> StubJoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let tracker = PendingTracker::new(Arc::clone(&self.pending));
        let start = Instant::now();
        let instrumented = async move {
            foundation_metrics::histogram!(SPAWN_LATENCY_METRIC, start.elapsed().as_secs_f64());
            let _guard = tracker;
            future.await
        };
        StubJoinHandle::pending(Box::pin(instrumented))
    }

    pub(crate) fn spawn_blocking<F, R>(&self, func: F) -> StubJoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let tracker = PendingTracker::new(Arc::clone(&self.pending));
        let start = Instant::now();
        let output = func();
        foundation_metrics::histogram!(SPAWN_LATENCY_METRIC, start.elapsed().as_secs_f64());
        drop(tracker);
        StubJoinHandle::completed(output)
    }

    pub(crate) fn sleep(&self, duration: Duration) -> StubSleep {
        StubSleep::new(duration)
    }

    pub(crate) fn interval(&self, duration: Duration) -> StubInterval {
        StubInterval::new(duration)
    }
}

pub(crate) async fn timeout<F, T>(
    _runtime: &StubRuntime,
    duration: Duration,
    future: F,
) -> Result<T, crate::TimeoutError>
where
    F: Future<Output = T>,
{
    StubTimeoutFuture::new(future, duration).await
}

struct PendingTracker {
    pending: PendingCounter,
}

impl PendingTracker {
    fn new(counter: PendingCounter) -> Self {
        let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
        foundation_metrics::gauge!(PENDING_TASKS_METRIC, current as f64);
        Self { pending: counter }
    }
}

impl Drop for PendingTracker {
    fn drop(&mut self) {
        let remaining = self.pending.fetch_sub(1, Ordering::SeqCst) - 1;
        foundation_metrics::gauge!(PENDING_TASKS_METRIC, remaining as f64);
    }
}

pub(crate) struct StubJoinHandle<T> {
    state: Mutex<Option<StubJoinState<T>>>,
}

enum StubJoinState<T> {
    Pending(Pin<Box<dyn Future<Output = T> + Send>>),
    Ready(Result<T, StubJoinError>),
}

impl<T> StubJoinHandle<T> {
    fn pending(future: Pin<Box<dyn Future<Output = T> + Send>>) -> Self {
        Self {
            state: Mutex::new(Some(StubJoinState::Pending(future))),
        }
    }

    fn completed(value: T) -> Self {
        Self {
            state: Mutex::new(Some(StubJoinState::Ready(Ok(value)))),
        }
    }

    pub(crate) fn abort(&self) {
        let mut state = self.state.lock().expect("stub join handle poisoned");
        *state = Some(StubJoinState::Ready(Err(StubJoinError::cancelled())));
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<T, StubJoinError>> {
        let mut state = self.state.lock().expect("stub join handle poisoned");
        let current = state.take();
        drop(state);

        match current {
            Some(StubJoinState::Pending(mut future)) => match future.as_mut().poll(cx) {
                Poll::Ready(output) => Poll::Ready(Ok(output)),
                Poll::Pending => {
                    let mut state = self.state.lock().expect("stub join handle poisoned");
                    *state = Some(StubJoinState::Pending(future));
                    Poll::Pending
                }
            },
            Some(StubJoinState::Ready(result)) => Poll::Ready(result),
            None => Poll::Ready(Err(StubJoinError::double_poll())),
        }
    }
}

#[derive(Debug)]
pub(crate) struct StubJoinError {
    kind: StubJoinErrorKind,
}

#[derive(Debug)]
enum StubJoinErrorKind {
    DoublePoll,
    Cancelled,
}

impl StubJoinError {
    fn double_poll() -> Self {
        Self {
            kind: StubJoinErrorKind::DoublePoll,
        }
    }

    fn cancelled() -> Self {
        Self {
            kind: StubJoinErrorKind::Cancelled,
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        matches!(self.kind, StubJoinErrorKind::Cancelled)
    }

    pub(crate) fn is_panic(&self) -> bool {
        false
    }
}

impl fmt::Display for StubJoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            StubJoinErrorKind::DoublePoll => {
                write!(f, "stub runtime join handle polled after completion")
            }
            StubJoinErrorKind::Cancelled => write!(f, "stub runtime task aborted"),
        }
    }
}

impl std::error::Error for StubJoinError {}

pub(crate) struct StubSleep {
    deadline: Instant,
    scheduled: bool,
}

impl StubSleep {
    fn new(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            scheduled: false,
        }
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.deadline {
            Poll::Ready(())
        } else {
            if !self.scheduled {
                self.scheduled = true;
                let wake_at = self.deadline;
                let waker = cx.waker().clone();
                thread::spawn(move || {
                    let now = Instant::now();
                    if wake_at > now {
                        thread::sleep(wake_at - now);
                    }
                    waker.wake();
                });
            }
            Poll::Pending
        }
    }
}

pub(crate) struct StubInterval {
    period: Duration,
    next: Instant,
    scheduled: bool,
}

impl StubInterval {
    fn new(period: Duration) -> Self {
        let next = Instant::now() + period;
        Self {
            period,
            next,
            scheduled: false,
        }
    }

    pub(crate) async fn tick(&mut self) -> Instant {
        StubIntervalTick { interval: self }.await
    }

    fn schedule(&mut self, cx: &Context<'_>) {
        if self.scheduled {
            return;
        }

        self.scheduled = true;
        let wake_at = self.next;
        let waker = cx.waker().clone();
        thread::spawn(move || {
            let now = Instant::now();
            if wake_at > now {
                thread::sleep(wake_at - now);
            }
            waker.wake();
        });
    }
}

struct StubIntervalTick<'a> {
    interval: &'a mut StubInterval,
}

impl<'a> Unpin for StubIntervalTick<'a> {}

impl<'a> Future for StubIntervalTick<'a> {
    type Output = Instant;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let interval = self.get_mut();
        let interval = &mut *interval.interval;
        let now = Instant::now();
        if now >= interval.next {
            let tick_at = interval.next;
            interval.next = tick_at + interval.period;
            interval.scheduled = false;
            Poll::Ready(tick_at)
        } else {
            interval.schedule(cx);
            Poll::Pending
        }
    }
}

pin_project! {
    struct StubTimeoutFuture<F> {
        #[pin]
        future: F,
        #[pin]
        sleep: StubSleep,
        duration: Duration,
    }
}

impl<F> StubTimeoutFuture<F> {
    fn new(future: F, duration: Duration) -> Self {
        Self {
            future,
            sleep: StubSleep::new(duration),
            duration,
        }
    }
}

impl<F, T> Future for StubTimeoutFuture<F>
where
    F: Future<Output = T>,
{
    type Output = Result<T, crate::TimeoutError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if let Poll::Ready(output) = this.future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }

        match this.sleep.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(crate::TimeoutError::from(*this.duration))),
            Poll::Pending => Poll::Pending,
        }
    }
}
pub(crate) async fn yield_now() {
    std::thread::yield_now();
}

#[macro_export]
#[doc(hidden)]
macro_rules! __runtime_select_stub {
    (biased; $($rest:tt)*) => {{
        use futures::FutureExt;
        $crate::__runtime_select_stub!(@biased [] $($rest)*)
    }};
    ($($rest:tt)*) => {{
        use futures::FutureExt;
        $crate::__runtime_select_stub!(@unbiased [] $($rest)*)
    }};
    (@unbiased [$($prefix:tt)*] $pat:pat = $expr:expr => $body:block, $($rest:tt)*) => {
        $crate::__runtime_select_stub!(@unbiased [$($prefix)* $pat = ($expr).fuse() => $body,] $($rest)*)
    };
    (@unbiased [$($prefix:tt)*] default => $body:block $(,)?) => {
        futures::select! { $($prefix)* default => $body }
    };
    (@unbiased [$($prefix:tt)*] complete => $body:block $(,)?) => {
        futures::select! { $($prefix)* complete => $body }
    };
    (@unbiased [$($prefix:tt)*]) => {
        futures::select! { $($prefix)* }
    };
    (@biased [$($prefix:tt)*] $pat:pat = $expr:expr => $body:block, $($rest:tt)*) => {
        $crate::__runtime_select_stub!(@biased [$($prefix)* $pat = ($expr).fuse() => $body,] $($rest)*)
    };
    (@biased [$($prefix:tt)*] default => $body:block $(,)?) => {
        futures::select_biased! { $($prefix)* default => $body }
    };
    (@biased [$($prefix:tt)*] complete => $body:block $(,)?) => {
        futures::select_biased! { $($prefix)* complete => $body }
    };
    (@biased [$($prefix:tt)*]) => {
        futures::select_biased! { $($prefix)* }
    };
}

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use metrics::{gauge, histogram};
use tokio::runtime::Runtime;
use tokio::task;
use tokio::time;

type PendingCounter = Arc<AtomicI64>;

const SPAWN_LATENCY_METRIC: &str = "runtime_spawn_latency_seconds";
const PENDING_TASKS_METRIC: &str = "runtime_pending_tasks";

pub(crate) struct TokioRuntime {
    runtime: Runtime,
    pending: PendingCounter,
}

pub(crate) fn runtime() -> Arc<TokioRuntime> {
    Arc::new(TokioRuntime {
        runtime: Runtime::new()
            .unwrap_or_else(|err| panic!("failed to create tokio runtime: {err}")),
        pending: Arc::new(AtomicI64::new(0)),
    })
}

pub(crate) async fn yield_now() {
    task::yield_now().await;
}

impl TokioRuntime {
    pub(crate) fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        self.runtime.block_on(future)
    }

    pub(crate) fn spawn<F, T>(&self, future: F) -> TokioJoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let tracker = PendingTracker::new(Arc::clone(&self.pending));
        let start = Instant::now();
        let instrumented = async move {
            histogram!(SPAWN_LATENCY_METRIC, start.elapsed().as_secs_f64());
            let _guard = tracker;
            future.await
        };

        TokioJoinHandle::new(self.runtime.spawn(instrumented))
    }

    pub(crate) fn spawn_blocking<F, R>(&self, func: F) -> TokioJoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let tracker = PendingTracker::new(Arc::clone(&self.pending));
        let start = Instant::now();
        let handle = self.runtime.spawn_blocking(move || {
            histogram!(SPAWN_LATENCY_METRIC, start.elapsed().as_secs_f64());
            let _guard = tracker;
            func()
        });
        TokioJoinHandle::new(handle)
    }

    pub(crate) fn sleep(&self, duration: Duration) -> TokioSleep {
        TokioSleep::new(duration)
    }

    pub(crate) fn interval(&self, duration: Duration) -> TokioInterval {
        TokioInterval {
            inner: time::interval(duration),
        }
    }
}

pub(crate) async fn timeout<F, T>(duration: Duration, future: F) -> Result<T, crate::TimeoutError>
where
    F: Future<Output = T>,
{
    match time::timeout(duration, future).await {
        Ok(val) => Ok(val),
        Err(_) => Err(crate::TimeoutError::from(duration)),
    }
}

struct PendingTracker {
    pending: PendingCounter,
}

impl PendingTracker {
    fn new(counter: PendingCounter) -> Self {
        let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
        gauge!(PENDING_TASKS_METRIC, current as f64);
        Self { pending: counter }
    }
}

impl Drop for PendingTracker {
    fn drop(&mut self) {
        let remaining = self.pending.fetch_sub(1, Ordering::SeqCst) - 1;
        gauge!(PENDING_TASKS_METRIC, remaining as f64);
    }
}

pub(crate) struct TokioJoinHandle<T> {
    inner: task::JoinHandle<T>,
}

impl<T> TokioJoinHandle<T> {
    fn new(inner: task::JoinHandle<T>) -> Self {
        Self { inner }
    }

    pub(crate) fn abort(&self) {
        self.inner.abort();
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<T, TokioJoinError>> {
        match Pin::new(&mut self.inner).poll(cx) {
            Poll::Ready(res) => Poll::Ready(res.map_err(Into::into)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Debug)]
pub(crate) struct TokioJoinError {
    inner: task::JoinError,
}

impl TokioJoinError {
    fn new(inner: task::JoinError) -> Self {
        Self { inner }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    pub(crate) fn is_panic(&self) -> bool {
        self.inner.is_panic()
    }
}

impl From<task::JoinError> for TokioJoinError {
    fn from(value: task::JoinError) -> Self {
        TokioJoinError::new(value)
    }
}

impl fmt::Display for TokioJoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl std::error::Error for TokioJoinError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}

pub(crate) struct TokioSleep {
    inner: Pin<Box<time::Sleep>>,
}

impl TokioSleep {
    pub(crate) fn new(duration: Duration) -> Self {
        Self {
            inner: Box::pin(time::sleep(duration)),
        }
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.inner).poll(cx)
    }
}

pub(crate) struct TokioInterval {
    inner: time::Interval,
}

impl TokioInterval {
    pub(crate) async fn tick(&mut self) -> std::time::Instant {
        self.inner.tick().await.into_std()
    }
}

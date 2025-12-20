#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use concurrency::Lazy;
use std::env;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

#[cfg(feature = "inhouse-backend")]
mod inhouse;
#[cfg(feature = "stub-backend")]
mod stub_impl;

pub mod fs;
pub mod io;
pub mod net;
pub mod sync;
pub mod telemetry;
pub mod ws;

pub use foundation_async::future::{join_all, select2, Either};

#[cfg(not(any(feature = "stub-backend", feature = "inhouse-backend")))]
compile_error!("At least one runtime backend must be enabled for crates/runtime");

// Note: When both features are enabled (e.g., via --all-features), inhouse-backend takes precedence.
// This is handled correctly by the select_backend() function which checks inhouse-backend first.

static GLOBAL_HANDLE: Lazy<RuntimeHandle> = Lazy::new(RuntimeHandle::bootstrap);

/// Handle to the active async runtime backend.
#[derive(Clone)]
pub struct RuntimeHandle {
    inner: BackendHandle,
}

#[derive(Clone)]
enum BackendHandle {
    #[cfg(feature = "inhouse-backend")]
    InHouse(Arc<inhouse::InHouseRuntime>),
    #[cfg(feature = "stub-backend")]
    Stub(Arc<stub_impl::StubRuntime>),
}
/// Returns the set of runtime backends that were compiled into the crate.
pub fn compiled_backends() -> &'static [&'static str] {
    const BACKENDS: &[&str] = &[
        #[cfg(feature = "inhouse-backend")]
        "inhouse",
        #[cfg(feature = "stub-backend")]
        "stub",
    ];
    BACKENDS
}

/// Error returned when a task join fails.
#[derive(Debug)]
pub struct JoinError(JoinErrorKind);

#[derive(Debug)]
enum JoinErrorKind {
    #[cfg(feature = "inhouse-backend")]
    InHouse(inhouse::InHouseJoinError),
    #[cfg(feature = "stub-backend")]
    Stub(stub_impl::StubJoinError),
}

/// Error returned when a timeout elapses before a future completes.
#[derive(Clone, Debug)]
pub struct TimeoutError {
    duration: Duration,
}

/// Join handle returned from [`spawn`] and [`spawn_blocking`].
pub struct JoinHandle<T> {
    inner: JoinHandleInner<T>,
}

#[cfg(all(feature = "inhouse-backend", feature = "stub-backend"))]
enum JoinHandleInner<T> {
    InHouse(inhouse::InHouseJoinHandle<T>),
    Stub(stub_impl::StubJoinHandle<T>),
}

#[cfg(all(feature = "inhouse-backend", not(feature = "stub-backend")))]
enum JoinHandleInner<T> {
    InHouse(inhouse::InHouseJoinHandle<T>),
}

#[cfg(all(not(feature = "inhouse-backend"), feature = "stub-backend"))]
enum JoinHandleInner<T> {
    Stub(stub_impl::StubJoinHandle<T>),
}

/// Sleep future returned by [`sleep`].
pub struct Sleep {
    inner: SleepInner,
}

#[cfg(all(feature = "inhouse-backend", feature = "stub-backend"))]
enum SleepInner {
    InHouse(inhouse::InHouseSleep),
    Stub(stub_impl::StubSleep),
}

#[cfg(all(feature = "inhouse-backend", not(feature = "stub-backend")))]
enum SleepInner {
    InHouse(inhouse::InHouseSleep),
}

#[cfg(all(not(feature = "inhouse-backend"), feature = "stub-backend"))]
enum SleepInner {
    Stub(stub_impl::StubSleep),
}

/// Interval timer returned by [`interval`].
pub struct Interval {
    inner: IntervalInner,
}

enum IntervalInner {
    #[cfg(feature = "inhouse-backend")]
    InHouse(inhouse::InHouseInterval),
    #[cfg(feature = "stub-backend")]
    Stub(stub_impl::StubInterval),
}

impl RuntimeHandle {
    fn bootstrap() -> Self {
        let backend = select_backend();
        Self { inner: backend }
    }

    /// Resolve the backend that is currently active.
    pub fn backend_name(&self) -> &'static str {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(_) => "inhouse",
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(_) => "stub",
        }
    }

    pub fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(rt) => rt.block_on(future),
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(rt) => rt.block_on(future),
        }
    }

    pub fn spawn<F, T>(&self, future: F) -> JoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(rt) => JoinHandle {
                inner: JoinHandleInner::InHouse(rt.spawn(future)),
            },
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(rt) => JoinHandle {
                inner: JoinHandleInner::Stub(rt.spawn(future)),
            },
        }
    }

    pub fn spawn_blocking<F, R>(&self, func: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(rt) => JoinHandle {
                inner: JoinHandleInner::InHouse(rt.spawn_blocking(func)),
            },
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(rt) => JoinHandle {
                inner: JoinHandleInner::Stub(rt.spawn_blocking(func)),
            },
        }
    }

    pub fn sleep(&self, duration: Duration) -> Sleep {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(rt) => Sleep {
                inner: SleepInner::InHouse(rt.sleep(duration)),
            },
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(rt) => Sleep {
                inner: SleepInner::Stub(rt.sleep(duration)),
            },
        }
    }

    pub fn interval(&self, duration: Duration) -> Interval {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(rt) => Interval {
                inner: IntervalInner::InHouse(rt.interval(duration)),
            },
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(rt) => Interval {
                inner: IntervalInner::Stub(rt.interval(duration)),
            },
        }
    }

    pub async fn yield_now(&self) {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(_) => inhouse::yield_now().await,
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(_) => stub_impl::yield_now().await,
        }
    }

    #[cfg(feature = "inhouse-backend")]
    pub(crate) fn inhouse_runtime(&self) -> Option<Arc<inhouse::InHouseRuntime>> {
        #[cfg(feature = "stub-backend")]
        {
            match &self.inner {
                BackendHandle::InHouse(rt) => Some(Arc::clone(rt)),
                BackendHandle::Stub(_) => None,
            }
        }

        #[cfg(not(feature = "stub-backend"))]
        {
            match &self.inner {
                BackendHandle::InHouse(rt) => Some(Arc::clone(rt)),
            }
        }
    }

    pub async fn timeout<F, T>(&self, duration: Duration, future: F) -> Result<T, TimeoutError>
    where
        F: Future<Output = T>,
    {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            BackendHandle::InHouse(rt) => inhouse::timeout(rt, duration, future).await,
            #[cfg(feature = "stub-backend")]
            BackendHandle::Stub(rt) => stub_impl::timeout(rt, duration, future).await,
        }
    }
}

fn select_backend() -> BackendHandle {
    let requested = env::var("TB_RUNTIME_BACKEND").ok();
    let requested = requested
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    match requested.as_deref() {
        #[cfg(feature = "inhouse-backend")]
        Some("inhouse") => return BackendHandle::InHouse(inhouse::runtime()),
        #[cfg(feature = "stub-backend")]
        Some("stub") => return BackendHandle::Stub(stub_impl::runtime()),
        #[cfg(not(feature = "inhouse-backend"))]
        Some("inhouse") => eprintln!(
            "TB_RUNTIME_BACKEND=inhouse requested but in-house backend not compiled; using fallback",
        ),
        #[cfg(not(feature = "stub-backend"))]
        Some("stub") => eprintln!(
            "TB_RUNTIME_BACKEND=stub requested but stub backend not compiled; using fallback",
        ),
        Some(other) => {
            eprintln!(
                "TB_RUNTIME_BACKEND={} is unknown; falling back to default backend",
                other
            );
        }
        None => {}
    }

    #[cfg(feature = "inhouse-backend")]
    let backend = BackendHandle::InHouse(inhouse::runtime());

    #[cfg(all(not(feature = "inhouse-backend"), feature = "stub-backend",))]
    let backend = BackendHandle::Stub(stub_impl::runtime());

    backend
}

/// Returns a clone of the process-global runtime handle.
pub fn handle() -> RuntimeHandle {
    GLOBAL_HANDLE.clone()
}

/// Runs a future to completion on the selected backend.
pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    handle().block_on(future)
}

/// Spawns an asynchronous task on the active backend.
pub fn spawn<F, T>(future: F) -> JoinHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    handle().spawn(future)
}

/// Executes a blocking function on a dedicated thread pool.
pub fn spawn_blocking<F, R>(func: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    handle().spawn_blocking(func)
}

/// Returns a future that completes after the specified duration elapses.
pub fn sleep(duration: Duration) -> Sleep {
    handle().sleep(duration)
}

/// Creates a periodic timer producing ticks spaced by the provided duration.
pub fn interval(duration: Duration) -> Interval {
    handle().interval(duration)
}

/// Yields execution to allow other tasks to make progress on the active backend.
pub async fn yield_now() {
    handle().yield_now().await
}

/// Awaits a future until the timeout expires.
pub async fn timeout<F, T>(duration: Duration, future: F) -> Result<T, TimeoutError>
where
    F: Future<Output = T>,
{
    handle().timeout(duration, future).await
}

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            #[cfg(feature = "inhouse-backend")]
            JoinErrorKind::InHouse(err) => write!(f, "{err}"),
            #[cfg(feature = "stub-backend")]
            JoinErrorKind::Stub(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for JoinError {}

impl JoinError {
    pub fn is_cancelled(&self) -> bool {
        match &self.0 {
            #[cfg(feature = "inhouse-backend")]
            JoinErrorKind::InHouse(err) => err.is_cancelled(),
            #[cfg(feature = "stub-backend")]
            JoinErrorKind::Stub(err) => err.is_cancelled(),
        }
    }

    pub fn is_panic(&self) -> bool {
        match &self.0 {
            #[cfg(feature = "inhouse-backend")]
            JoinErrorKind::InHouse(err) => err.is_panic(),
            #[cfg(feature = "stub-backend")]
            JoinErrorKind::Stub(err) => err.is_panic(),
        }
    }
}

impl<T> JoinHandle<T> {
    /// Cancels the task without waiting for it to finish.
    pub fn abort(&self) {
        match &self.inner {
            #[cfg(feature = "inhouse-backend")]
            JoinHandleInner::InHouse(handle) => handle.abort(),
            #[cfg(feature = "stub-backend")]
            JoinHandleInner::Stub(handle) => handle.abort(),
        }
    }
}

impl<T> Future for JoinHandle<T>
where
    T: Send + 'static + Unpin,
{
    type Output = Result<T, JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match &mut this.inner {
            #[cfg(feature = "inhouse-backend")]
            JoinHandleInner::InHouse(handle) => handle.poll(cx).map(|res| res.map_err(Into::into)),
            #[cfg(feature = "stub-backend")]
            JoinHandleInner::Stub(handle) => handle.poll(cx).map(|res| res.map_err(Into::into)),
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match &mut this.inner {
            #[cfg(feature = "inhouse-backend")]
            SleepInner::InHouse(handle) => handle.poll(cx),
            #[cfg(feature = "stub-backend")]
            SleepInner::Stub(handle) => handle.poll(cx),
        }
    }
}

impl Interval {
    pub async fn tick(&mut self) -> std::time::Instant {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            IntervalInner::InHouse(interval) => interval.tick().await,
            #[cfg(feature = "stub-backend")]
            IntervalInner::Stub(interval) => interval.tick().await,
        }
    }
}

impl From<Duration> for TimeoutError {
    fn from(duration: Duration) -> Self {
        Self { duration }
    }
}

impl fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "operation timed out after {:?}", self.duration)
    }
}

impl std::error::Error for TimeoutError {}

#[cfg(feature = "stub-backend")]
impl From<stub_impl::StubJoinError> for JoinError {
    fn from(err: stub_impl::StubJoinError) -> Self {
        JoinError(JoinErrorKind::Stub(err))
    }
}

#[cfg(feature = "inhouse-backend")]
impl From<inhouse::InHouseJoinError> for JoinError {
    fn from(err: inhouse::InHouseJoinError) -> Self {
        JoinError(JoinErrorKind::InHouse(err))
    }
}

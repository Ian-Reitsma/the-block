use foundation_async::future::catch_unwind;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::panic::{self, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, LockResult, Mutex, OnceLock, Weak};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use crate::sync::oneshot;
use foundation_async::block_on;
use std::io;
use sys::reactor::{
    Event as ReactorEvent, Events as ReactorEvents, Interest as ReactorInterest,
    Poll as ReactorPoll, Token, Waker as ReactorWaker,
};

#[cfg(unix)]
pub(crate) type ReactorRaw = std::os::fd::RawFd;
#[cfg(target_os = "windows")]
pub(crate) type ReactorRaw = std::os::windows::io::RawSocket;

pub(crate) mod net;

const SPAWN_LATENCY_METRIC: &str = "runtime_spawn_latency_seconds";
const PENDING_TASKS_METRIC: &str = "runtime_pending_tasks";
// Use a 32-bit sentinel for compatibility with pollers that truncate user data
// fields (observed in some epoll configurations). Keeping this value within
// the u32 range ensures token comparisons remain stable across platforms.
const REACTOR_WAKER_TOKEN: Token = Token(u32::MAX as usize - 1);
const DEFAULT_REACTOR_IDLE_POLL_MS: u64 = 100;
const DEFAULT_IO_READ_BACKOFF_MS: u64 = 10;
const DEFAULT_IO_WRITE_BACKOFF_MS: u64 = 10;

static REACTOR_IDLE_POLL_MS: AtomicU64 = AtomicU64::new(DEFAULT_REACTOR_IDLE_POLL_MS);
static IO_READ_BACKOFF_MS: AtomicU64 = AtomicU64::new(DEFAULT_IO_READ_BACKOFF_MS);
static IO_WRITE_BACKOFF_MS: AtomicU64 = AtomicU64::new(DEFAULT_IO_WRITE_BACKOFF_MS);

pub(crate) fn set_reactor_idle_poll(duration: Duration) {
    let millis = duration.as_millis().min(u64::MAX as u128) as u64;
    let value = millis.max(1);
    REACTOR_IDLE_POLL_MS.store(value, AtomicOrdering::SeqCst);
}

pub(crate) fn set_io_read_backoff(duration: Duration) {
    let millis = duration.as_millis().min(u64::MAX as u128) as u64;
    let value = millis.max(1);
    IO_READ_BACKOFF_MS.store(value, AtomicOrdering::SeqCst);
}

pub(crate) fn set_io_write_backoff(duration: Duration) {
    let millis = duration.as_millis().min(u64::MAX as u128) as u64;
    let value = millis.max(1);
    IO_WRITE_BACKOFF_MS.store(value, AtomicOrdering::SeqCst);
}

fn reactor_idle_poll() -> Duration {
    Duration::from_millis(REACTOR_IDLE_POLL_MS.load(AtomicOrdering::SeqCst).max(1))
}

pub(crate) fn io_read_backoff() -> Duration {
    Duration::from_millis(IO_READ_BACKOFF_MS.load(AtomicOrdering::SeqCst).max(1))
}

pub(crate) fn io_write_backoff() -> Duration {
    Duration::from_millis(IO_WRITE_BACKOFF_MS.load(AtomicOrdering::SeqCst).max(1))
}

fn reactor_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("RUNTIME_REACTOR_DEBUG").is_ok())
}

pub(crate) struct InHouseRuntime {
    inner: Arc<Inner>,
}

type PendingCounter = Arc<AtomicI64>;

type TaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

type JoinSenderSlot<T> = Arc<Mutex<Option<oneshot::Sender<Result<T, InHouseJoinError>>>>>;

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

pub(crate) fn runtime() -> Arc<InHouseRuntime> {
    Arc::new(InHouseRuntime {
        inner: Inner::new(),
    })
}

impl InHouseRuntime {
    pub(crate) fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        block_on(future)
    }

    pub(crate) fn spawn<F, T>(&self, future: F) -> InHouseJoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let tracker = PendingTracker::new(Arc::clone(&self.inner.pending));
        let start = Instant::now();
        let (sender, receiver) = oneshot::channel();
        let sender_slot = Arc::new(Mutex::new(Some(sender)));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_for_task = Arc::clone(&cancel_flag);
        let sender_for_task = Arc::clone(&sender_slot);
        let instrumented = async move {
            foundation_metrics::histogram!(SPAWN_LATENCY_METRIC, start.elapsed().as_secs_f64());
            let _guard = tracker;
            let cancelable = CancelableFuture::new(future, Arc::clone(&cancel_for_task));
            let outcome = catch_unwind(AssertUnwindSafe(cancelable)).await;
            match outcome {
                Ok(CancelOutcome::Completed(value)) => {
                    if let Some(sender) = sender_for_task.lock().recover().take() {
                        let _ = sender.send(Ok(value));
                    }
                }
                Ok(CancelOutcome::Cancelled) => {
                    if let Some(sender) = sender_for_task.lock().recover().take() {
                        let _ = sender.send(Err(InHouseJoinError::cancelled()));
                    }
                }
                Err(_) => {
                    if let Some(sender) = sender_for_task.lock().recover().take() {
                        let _ = sender.send(Err(InHouseJoinError::panic()));
                    }
                }
            }
        };

        let task = Task::spawn(Arc::clone(&self.inner), Box::pin(instrumented));

        InHouseJoinHandle::new(receiver, sender_slot, cancel_flag, Some(task))
    }

    pub(crate) fn spawn_blocking<F, R>(&self, func: F) -> InHouseJoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let tracker = PendingTracker::new(Arc::clone(&self.inner.pending));
        let start = Instant::now();
        let (sender, receiver) = oneshot::channel();
        let sender_slot = Arc::new(Mutex::new(Some(sender)));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_for_job = Arc::clone(&cancel_flag);
        let sender_for_job = Arc::clone(&sender_slot);

        self.inner.blocking.spawn(BlockingJob::new(move || {
            foundation_metrics::histogram!(SPAWN_LATENCY_METRIC, start.elapsed().as_secs_f64());
            let _guard = tracker;
            if cancel_for_job.load(AtomicOrdering::SeqCst) {
                if let Some(sender) = sender_for_job.lock().recover().take() {
                    let _ = sender.send(Err(InHouseJoinError::cancelled()));
                }
                return;
            }

            match panic::catch_unwind(AssertUnwindSafe(func)) {
                Ok(value) => {
                    if let Some(sender) = sender_for_job.lock().recover().take() {
                        let _ = sender.send(Ok(value));
                    }
                }
                Err(_) => {
                    if let Some(sender) = sender_for_job.lock().recover().take() {
                        let _ = sender.send(Err(InHouseJoinError::panic()));
                    }
                }
            }
        }));

        InHouseJoinHandle::new(receiver, sender_slot, cancel_flag, None)
    }

    pub(crate) fn sleep(&self, duration: Duration) -> InHouseSleep {
        InHouseSleep::new(Arc::clone(&self.inner.reactor), duration)
    }

    pub(crate) fn interval(&self, duration: Duration) -> InHouseInterval {
        InHouseInterval::new(Arc::clone(&self.inner.reactor), duration)
    }

    pub(crate) fn reactor(&self) -> Arc<ReactorInner> {
        Arc::clone(&self.inner.reactor)
    }
}

pub(crate) async fn timeout<F, T>(
    runtime: &InHouseRuntime,
    duration: Duration,
    future: F,
) -> Result<T, crate::TimeoutError>
where
    F: Future<Output = T>,
{
    InHouseTimeoutFuture::new(future, runtime.sleep(duration), duration).await
}

struct PendingTracker {
    pending: PendingCounter,
}

impl PendingTracker {
    fn new(counter: PendingCounter) -> Self {
        let current = counter.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        foundation_metrics::gauge!(PENDING_TASKS_METRIC, current as f64);
        Self { pending: counter }
    }
}

impl Drop for PendingTracker {
    fn drop(&mut self) {
        let remaining = self.pending.fetch_sub(1, AtomicOrdering::SeqCst) - 1;
        foundation_metrics::gauge!(PENDING_TASKS_METRIC, remaining as f64);
    }
}

struct Inner {
    queue: WorkQueue<Arc<Task>>,
    shutdown: AtomicBool,
    pending: PendingCounter,
    reactor: Arc<ReactorInner>,
    blocking: BlockingPool,
    worker_handles: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl Inner {
    fn new() -> Arc<Self> {
        let worker_count = thread::available_parallelism()
            .map(|v| v.get())
            .unwrap_or(1)
            .max(2);
        let reactor = ReactorInner::new();
        let inner = Arc::new(Self {
            queue: WorkQueue::new(),
            shutdown: AtomicBool::new(false),
            pending: Arc::new(AtomicI64::new(0)),
            reactor,
            blocking: BlockingPool::new(worker_count.max(2)),
            worker_handles: Mutex::new(Vec::new()),
        });
        inner.spawn_workers(worker_count);
        inner
    }

    fn spawn_workers(self: &Arc<Self>, worker_count: usize) {
        let mut handles = self.worker_handles.lock().recover();
        for index in 0..worker_count {
            let runtime = Arc::clone(self);
            let queue = self.queue.clone();
            let handle = thread::Builder::new()
                .name(format!("inhouse-runtime-worker-{index}"))
                .spawn(move || {
                    SchedulerWorker::new(runtime, queue).run();
                })
                .unwrap_or_else(|err| panic!("failed to spawn in-house runtime worker: {err}"));
            handles.push(handle);
        }
    }

    fn schedule(&self, task: Arc<Task>) {
        self.queue.push(task);
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.shutdown.store(true, AtomicOrdering::SeqCst);
        self.queue.notify_all();
        self.reactor.shutdown();
        self.blocking.shutdown();
        let mut handles = self.worker_handles.lock().recover();
        for handle in handles.drain(..) {
            let _ = handle.join();
        }
    }
}

struct SchedulerWorker {
    runtime: Arc<Inner>,
    queue: WorkQueue<Arc<Task>>,
}

impl SchedulerWorker {
    fn new(runtime: Arc<Inner>, queue: WorkQueue<Arc<Task>>) -> Self {
        Self { runtime, queue }
    }

    fn run(self) {
        loop {
            if self.runtime.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }

            if let Some(task) = self.queue.pop(&self.runtime.shutdown) {
                task.run();
                continue;
            }

            if self.runtime.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }
        }
    }
}

struct Task {
    future: Mutex<Option<TaskFuture>>,
    scheduled: AtomicBool,
    runtime: Weak<Inner>,
}

impl Task {
    fn spawn(runtime: Arc<Inner>, future: TaskFuture) -> Arc<Self> {
        let task = Arc::new(Self {
            future: Mutex::new(Some(future)),
            scheduled: AtomicBool::new(false),
            runtime: Arc::downgrade(&runtime),
        });
        task.schedule();
        task
    }

    fn schedule(self: &Arc<Self>) {
        if !self.scheduled.swap(true, AtomicOrdering::SeqCst) {
            if let Some(runtime) = self.runtime.upgrade() {
                runtime.schedule(Arc::clone(self));
            }
        }
    }

    fn run(self: Arc<Self>) {
        self.scheduled.store(false, AtomicOrdering::SeqCst);
        let mut slot = self.future.lock().recover();
        if let Some(mut future) = slot.take() {
            let waker = Waker::from(Arc::clone(&self));
            let mut cx = Context::from_waker(&waker);
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {}
                Poll::Pending => {
                    *slot = Some(future);
                }
            }
        }
    }
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}

struct WorkQueue<T> {
    inner: Arc<WorkQueueInner<T>>,
}

struct WorkQueueInner<T> {
    queue: Mutex<VecDeque<T>>,
    cv: Condvar,
}

impl<T> Clone for WorkQueue<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> WorkQueue<T> {
    fn new() -> Self {
        Self {
            inner: Arc::new(WorkQueueInner {
                queue: Mutex::new(VecDeque::new()),
                cv: Condvar::new(),
            }),
        }
    }

    fn push(&self, item: T) {
        let mut guard = self.inner.queue.lock().recover();
        guard.push_back(item);
        self.inner.cv.notify_one();
    }

    fn pop(&self, shutdown: &AtomicBool) -> Option<T> {
        let mut guard = self.inner.queue.lock().recover();
        loop {
            if let Some(item) = guard.pop_front() {
                return Some(item);
            }

            if shutdown.load(AtomicOrdering::SeqCst) {
                return None;
            }

            guard = self.inner.cv.wait(guard).recover();
        }
    }

    fn notify_all(&self) {
        self.inner.cv.notify_all();
    }
}

struct BlockingPool {
    queue: WorkQueue<BlockingJob>,
    shutdown: Arc<AtomicBool>,
    workers: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl BlockingPool {
    fn new(worker_count: usize) -> Self {
        let queue = WorkQueue::new();
        let shutdown = Arc::new(AtomicBool::new(false));
        let handles = (0..worker_count)
            .map(|index| {
                let queue_clone = queue.clone();
                let shutdown_clone = Arc::clone(&shutdown);
                thread::Builder::new()
                    .name(format!("inhouse-blocking-worker-{index}"))
                    .spawn(move || {
                        BlockingWorker::new(queue_clone, shutdown_clone).run();
                    })
                    .unwrap_or_else(|err| panic!("failed to spawn in-house blocking worker: {err}"))
            })
            .collect();
        Self {
            queue,
            shutdown,
            workers: Mutex::new(handles),
        }
    }

    fn spawn(&self, job: BlockingJob) {
        self.queue.push(job);
    }

    fn shutdown(&self) {
        if self.shutdown.swap(true, AtomicOrdering::SeqCst) {
            return;
        }
        self.queue.notify_all();
        let mut handles = self.workers.lock().recover();
        for handle in handles.drain(..) {
            let _ = handle.join();
        }
    }
}

struct BlockingWorker {
    queue: WorkQueue<BlockingJob>,
    shutdown: Arc<AtomicBool>,
}

impl BlockingWorker {
    fn new(queue: WorkQueue<BlockingJob>, shutdown: Arc<AtomicBool>) -> Self {
        Self { queue, shutdown }
    }

    fn run(self) {
        loop {
            if self.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }

            if let Some(job) = self.queue.pop(&self.shutdown) {
                job.run();
                continue;
            }

            if self.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }
        }
    }
}

struct BlockingJob {
    task: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl BlockingJob {
    fn new<F>(job: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self {
            task: Some(Box::new(job)),
        }
    }

    fn run(mut self) {
        if let Some(job) = self.task.take() {
            job();
        }
    }
}

#[derive(Clone)]
struct TimerRegistration {
    id: u64,
    reactor: Arc<ReactorInner>,
}

impl TimerRegistration {
    fn update_waker(&self, waker: &Waker) {
        self.reactor.update_waker(self.id, waker.clone());
    }

    fn cancel(&self) {
        self.reactor.cancel(self.id);
    }
}

pub(crate) struct ReactorInner {
    poll: ReactorPoll,
    waker: ReactorWaker,
    timers: Mutex<TimerState>,
    io: Mutex<IoState>,
    next_token: AtomicU64,
    shutdown: AtomicBool,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl ReactorInner {
    fn new() -> Arc<Self> {
        let poll = ReactorPoll::new()
            .unwrap_or_else(|err| panic!("failed to create reactor poller: {err}"));
        let waker = poll
            .create_waker(REACTOR_WAKER_TOKEN)
            .unwrap_or_else(|err| panic!("failed to create reactor waker: {err}"));
        let inner = Arc::new(Self {
            poll,
            waker,
            timers: Mutex::new(TimerState::new()),
            io: Mutex::new(IoState::new()),
            next_token: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            thread: Mutex::new(None),
        });
        let reactor_clone = Arc::clone(&inner);
        let handle = thread::Builder::new()
            .name("inhouse-reactor".into())
            .spawn(move || reactor_clone.run())
            .unwrap_or_else(|err| panic!("failed to spawn in-house reactor: {err}"));
        *inner.thread.lock().recover() = Some(handle);
        inner
    }

    fn run(self: Arc<Self>) {
        let mut events = ReactorEvents::with_capacity(128);
        while !self.shutdown.load(AtomicOrdering::SeqCst) {
            let timeout = self.compute_timeout();
            let _ = self.poll.poll(&mut events, timeout);
            for event in events.iter() {
                if event.token() == REACTOR_WAKER_TOKEN {
                    // drain wake-up notification
                    continue;
                }
                self.dispatch_io_event(event);
            }
            self.fire_due_timers();
        }
    }

    fn compute_timeout(&self) -> Option<Duration> {
        let mut state = self.timers.lock().recover();
        state.prune();
        let next = state
            .peek_deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let idle = reactor_idle_poll();
        Some(match next {
            Some(timeout) => timeout.min(idle),
            None => idle,
        })
    }

    fn fire_due_timers(&self) {
        loop {
            let maybe_waker = {
                let mut state = self.timers.lock().recover();
                state.pop_due()
            };
            match maybe_waker {
                Some(waker) => waker.wake(),
                None => break,
            }
        }
    }

    fn dispatch_io_event(&self, event: &ReactorEvent) {
        let token_index = event.token().0;
        let event_ident = event.ident().map(|ident| ident as ReactorRaw);
        let debug = reactor_debug_enabled();
        if debug {
            eprintln!(
                "[REACTOR] token={} ident={:?} read={} write={} err={} read_closed={} write_closed={} priority={}",
                token_index,
                event_ident,
                event.is_readable(),
                event.is_writable(),
                event.is_error(),
                event.is_read_closed(),
                event.is_write_closed(),
                event.is_priority()
            );
        }
        let mut wake_read = None;
        let mut wake_write = None;
        let mut wake_read_token = token_index;

        {
            let mut state = self.io.lock().recover();
            let mut resolved_token = token_index;
            let mut remap_reason: Option<&'static str> = None;
            if let (Some(fd), false) = (event_ident, event.is_priority()) {
                match state.entries.get(&resolved_token).map(|entry| entry.fd) {
                    Some(entry_fd) if entry_fd == fd => {}
                    Some(_) => {
                        remap_reason = Some("mismatched fd");
                    }
                    None => {
                        remap_reason = Some("missing token");
                    }
                }
                if remap_reason.is_some() {
                    if let Some(token) = state.token_by_fd.get(&fd).copied() {
                        resolved_token = token.0;
                        if debug {
                            eprintln!(
                                "[REACTOR] token={} {} remapped to {} via fd {:?}",
                                token_index,
                                remap_reason.unwrap_or(""),
                                resolved_token,
                                fd
                            );
                        }
                    }
                }
            }

            if let Some(entry) = state.entries.get_mut(&resolved_token) {
                wake_read_token = resolved_token;
                if event.is_readable()
                    || event.is_read_closed()
                    || event.is_error()
                    || event.is_priority()
                {
                    entry.read_ready = true;
                    wake_read = entry.read_waker.take();
                    if debug {
                        eprintln!(
                            "[REACTOR] token={} read_ready=true, has_waker={}",
                            wake_read_token,
                            wake_read.is_some()
                        );
                    }
                }
                if event.is_writable()
                    || event.is_write_closed()
                    || event.is_error()
                    || event.is_priority()
                {
                    entry.write_ready = true;
                    wake_write = entry.write_waker.take();
                }
            } else if debug {
                eprintln!(
                    "[REACTOR] token={} dropped event (ident={:?})",
                    token_index, event_ident
                );
            }
        }

        if let Some(waker) = wake_read {
            if debug {
                eprintln!("[REACTOR] token={} waking read task", wake_read_token);
            }
            waker.wake();
        }
        if let Some(waker) = wake_write {
            waker.wake();
        }
    }

    fn register_fd(&self, fd: ReactorRaw, interest: ReactorInterest) -> io::Result<Token> {
        let token_value = self.next_token.fetch_add(1, AtomicOrdering::SeqCst) as usize;
        let token = Token(token_value);
        {
            let mut state = self.io.lock().recover();
            state.insert(token, fd, interest);
        }
        if let Err(err) = self.poll.register(fd, token, interest) {
            let mut state = self.io.lock().recover();
            state.remove(token);
            return Err(err);
        }
        if reactor_debug_enabled() {
            eprintln!(
                "[REACTOR] register fd={} token={} read={} write={}",
                fd,
                token_value,
                interest.contains(ReactorInterest::READABLE),
                interest.contains(ReactorInterest::WRITABLE)
            );
        }
        let _ = self.waker.wake();
        Ok(token)
    }

    fn deregister_fd(&self, fd: ReactorRaw, token: Token) -> io::Result<()> {
        let _ = self.waker.wake();
        self.poll.deregister(fd, token)?;
        let mut state = self.io.lock().recover();
        state.remove(token);
        if reactor_debug_enabled() {
            eprintln!("[REACTOR] deregister fd={} token={}", fd, token.0);
        }
        Ok(())
    }

    fn poll_io_ready(
        &self,
        token: Token,
        direction: IoDirection,
        cx: &Context<'_>,
    ) -> Poll<io::Result<()>> {
        {
            let mut state = self.io.lock().recover();
            let entry = match state.entries.get_mut(&token.0) {
                Some(entry) => entry,
                None => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "io registration missing",
                    )))
                }
            };

            match direction {
                IoDirection::Read => {
                    if entry.read_ready {
                        entry.read_ready = false;
                        return Poll::Ready(Ok(()));
                    }
                    entry.read_waker = Some(cx.waker().clone());
                }
                IoDirection::Write => {
                    if entry.write_ready {
                        entry.write_ready = false;
                        return Poll::Ready(Ok(()));
                    }
                    entry.write_waker = Some(cx.waker().clone());
                }
            }
        }

        let _ = self.waker.wake();
        Poll::Pending
    }

    fn poll_read_ready(&self, token: Token, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.poll_io_ready(token, IoDirection::Read, cx)
    }

    fn poll_write_ready(&self, token: Token, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.poll_io_ready(token, IoDirection::Write, cx)
    }

    fn enable_write_interest(&self, token: Token) -> io::Result<()> {
        let (fd, prev_interest, new_interest, should_update) = {
            let mut state = self.io.lock().recover();
            let entry = state.entries.get_mut(&token.0).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "io registration missing")
            })?;
            entry.write_waiters = entry.write_waiters.saturating_add(1);
            let prev_interest = entry.interest;
            let mut new_interest = entry.interest;
            let mut should_update = false;
            if !entry.interest.contains(ReactorInterest::WRITABLE) {
                new_interest = entry.interest.union(ReactorInterest::WRITABLE);
                entry.interest = new_interest;
                should_update = true;
            }
            (entry.fd, prev_interest, new_interest, should_update)
        };

        if should_update {
            if let Err(err) = self
                .poll
                .update_interest(fd, token, prev_interest, new_interest)
            {
                let mut state = self.io.lock().recover();
                if let Some(entry) = state.entries.get_mut(&token.0) {
                    entry.interest = prev_interest;
                    entry.write_waiters = entry.write_waiters.saturating_sub(1);
                }
                return Err(err);
            }
        }
        Ok(())
    }

    fn disable_write_interest(&self, token: Token) -> io::Result<()> {
        let (fd, prev_interest, new_interest, should_update, decremented) = {
            let mut state = self.io.lock().recover();
            let entry = state.entries.get_mut(&token.0).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "io registration missing")
            })?;
            let mut decremented = false;
            if entry.write_waiters > 0 {
                entry.write_waiters -= 1;
                decremented = true;
            }
            let prev_interest = entry.interest;
            let mut new_interest = entry.interest;
            let mut should_update = false;
            if entry.write_waiters == 0 && entry.interest.contains(ReactorInterest::WRITABLE) {
                new_interest = entry.interest.without(ReactorInterest::WRITABLE);
                entry.interest = new_interest;
                should_update = true;
            }
            (
                entry.fd,
                prev_interest,
                new_interest,
                should_update,
                decremented,
            )
        };

        if should_update {
            if let Err(err) = self
                .poll
                .update_interest(fd, token, prev_interest, new_interest)
            {
                let mut state = self.io.lock().recover();
                if let Some(entry) = state.entries.get_mut(&token.0) {
                    entry.interest = prev_interest;
                    if decremented {
                        entry.write_waiters = entry.write_waiters.saturating_add(1);
                    }
                }
                return Err(err);
            }
        }
        Ok(())
    }

    fn register_timer(
        self: &Arc<Self>,
        deadline: Instant,
        waker: Waker,
        interval: Option<Duration>,
    ) -> TimerRegistration {
        let id = self.next_token.fetch_add(1, AtomicOrdering::SeqCst);
        {
            let mut state = self.timers.lock().recover();
            state.insert(id, deadline, waker, interval);
        }
        let _ = self.waker.wake();
        TimerRegistration {
            id,
            reactor: Arc::clone(self),
        }
    }

    fn update_waker(&self, id: u64, waker: Waker) {
        let mut state = self.timers.lock().recover();
        state.update_waker(id, waker);
    }

    fn cancel(&self, id: u64) {
        let mut state = self.timers.lock().recover();
        state.cancel(id);
    }

    fn shutdown(&self) {
        if self.shutdown.swap(true, AtomicOrdering::SeqCst) {
            return;
        }
        let _ = self.waker.wake();
        if let Some(handle) = self.thread.lock().recover().take() {
            let _ = handle.join();
        }
    }
}

struct TimerState {
    heap: BinaryHeap<TimerHeapEntry>,
    entries: HashMap<u64, TimerEntry>,
}

struct IoState {
    entries: HashMap<usize, IoEntry>,
    token_by_fd: HashMap<ReactorRaw, Token>,
}

impl IoState {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            token_by_fd: HashMap::new(),
        }
    }

    fn insert(&mut self, token: Token, fd: ReactorRaw, interest: ReactorInterest) {
        self.entries.insert(token.0, IoEntry::new(fd, interest));
        self.token_by_fd.insert(fd, token);
    }

    fn remove(&mut self, token: Token) {
        if let Some(entry) = self.entries.remove(&token.0) {
            self.token_by_fd.remove(&entry.fd);
        }
    }
}

struct IoEntry {
    fd: ReactorRaw,
    interest: ReactorInterest,
    read_ready: bool,
    write_ready: bool,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    write_waiters: usize,
}

impl IoEntry {
    fn new(fd: ReactorRaw, interest: ReactorInterest) -> Self {
        Self {
            fd,
            interest,
            read_ready: false,
            write_ready: false,
            read_waker: None,
            write_waker: None,
            write_waiters: if interest.contains(ReactorInterest::WRITABLE) {
                1
            } else {
                0
            },
        }
    }
}

enum IoDirection {
    Read,
    Write,
}

#[derive(Clone)]
pub(crate) struct IoRegistration {
    reactor: Arc<ReactorInner>,
    token: Token,
    fd: ReactorRaw,
}

impl IoRegistration {
    pub(crate) fn new(
        reactor: Arc<ReactorInner>,
        fd: ReactorRaw,
        interest: ReactorInterest,
    ) -> io::Result<Self> {
        let token = reactor.register_fd(fd, interest)?;
        Ok(Self { reactor, token, fd })
    }

    pub(crate) fn poll_read_ready(&self, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.reactor.poll_read_ready(self.token, cx)
    }

    pub(crate) fn poll_write_ready(&self, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.reactor.poll_write_ready(self.token, cx)
    }

    pub(crate) fn enable_write_interest(&self) -> io::Result<()> {
        self.reactor.enable_write_interest(self.token)
    }

    pub(crate) fn disable_write_interest(&self) -> io::Result<()> {
        self.reactor.disable_write_interest(self.token)
    }

    pub(crate) fn deregister(&self) -> io::Result<()> {
        self.reactor.deregister_fd(self.fd, self.token)
    }
}

impl TimerState {
    fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            entries: HashMap::new(),
        }
    }

    fn insert(&mut self, id: u64, deadline: Instant, waker: Waker, interval: Option<Duration>) {
        self.entries.insert(
            id,
            TimerEntry {
                deadline,
                waker,
                interval,
            },
        );
        self.heap.push(TimerHeapEntry { id, deadline });
    }

    fn update_waker(&mut self, id: u64, waker: Waker) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.waker = waker;
        }
    }

    fn cancel(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    fn prune(&mut self) {
        while let Some(entry) = self.heap.peek() {
            if self.entries.contains_key(&entry.id) {
                break;
            }
            self.heap.pop();
        }
    }

    fn peek_deadline(&mut self) -> Option<Instant> {
        loop {
            let entry = self.heap.peek()?;
            if let Some(timer) = self.entries.get(&entry.id) {
                return Some(timer.deadline);
            }
            self.heap.pop();
        }
    }

    fn pop_due(&mut self) -> Option<Waker> {
        loop {
            let entry = self.heap.peek()?;
            let now = Instant::now();
            if entry.deadline > now {
                return None;
            }
            let entry = self.heap.pop()?;
            if let Some(mut timer) = self.entries.remove(&entry.id) {
                if let Some(period) = timer.interval {
                    let next_deadline = entry.deadline + period;
                    timer.deadline = next_deadline;
                    let waker = timer.waker.clone();
                    self.entries.insert(entry.id, timer);
                    self.heap.push(TimerHeapEntry {
                        id: entry.id,
                        deadline: next_deadline,
                    });
                    return Some(waker);
                } else {
                    return Some(timer.waker);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct TimerHeapEntry {
    id: u64,
    deadline: Instant,
}

impl Ord for TimerHeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .deadline
            .cmp(&self.deadline)
            .then_with(|| other.id.cmp(&self.id))
    }
}

impl PartialOrd for TimerHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct TimerEntry {
    deadline: Instant,
    waker: Waker,
    interval: Option<Duration>,
}

pub(crate) struct InHouseSleep {
    deadline: Instant,
    handle: Option<TimerRegistration>,
    reactor: Arc<ReactorInner>,
}

impl InHouseSleep {
    fn new(reactor: Arc<ReactorInner>, duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            handle: None,
            reactor,
        }
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.deadline {
            if let Some(handle) = self.handle.take() {
                handle.cancel();
            }
            return Poll::Ready(());
        }

        match &mut self.handle {
            Some(handle) => handle.update_waker(cx.waker()),
            None => {
                let handle = self
                    .reactor
                    .register_timer(self.deadline, cx.waker().clone(), None);
                self.handle = Some(handle);
            }
        }
        Poll::Pending
    }
}

impl Drop for InHouseSleep {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.cancel();
        }
    }
}

pub(crate) struct InHouseInterval {
    period: Duration,
    next: Instant,
    handle: Option<TimerRegistration>,
    reactor: Arc<ReactorInner>,
}

impl InHouseInterval {
    fn new(reactor: Arc<ReactorInner>, period: Duration) -> Self {
        let next = Instant::now() + period;
        Self {
            period,
            next,
            handle: None,
            reactor,
        }
    }

    pub(crate) async fn tick(&mut self) -> Instant {
        IntervalTick { interval: self }.await
    }

    fn ensure_registered(&mut self, cx: &Context<'_>) {
        match &mut self.handle {
            Some(handle) => handle.update_waker(cx.waker()),
            None => {
                let handle =
                    self.reactor
                        .register_timer(self.next, cx.waker().clone(), Some(self.period));
                self.handle = Some(handle);
            }
        }
    }
}

impl Drop for InHouseInterval {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.cancel();
        }
    }
}

struct IntervalTick<'a> {
    interval: &'a mut InHouseInterval,
}

impl Future for IntervalTick<'_> {
    type Output = Instant;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if Instant::now() >= this.interval.next {
            let fired = this.interval.next;
            this.interval.next += this.interval.period;
            return Poll::Ready(fired);
        }
        this.interval.ensure_registered(cx);
        Poll::Pending
    }
}

struct InHouseTimeoutFuture<F> {
    future: Pin<Box<F>>,

    sleep: InHouseSleep,
    duration: Duration,
}

impl<F> InHouseTimeoutFuture<F> {
    fn new(future: F, sleep: InHouseSleep, duration: Duration) -> Self {
        Self {
            future: Box::pin(future),
            sleep,
            duration,
        }
    }
}

impl<F, T> Future for InHouseTimeoutFuture<F>
where
    F: Future<Output = T>,
{
    type Output = Result<T, crate::TimeoutError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Poll::Ready(output) = this.future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }

        match this.sleep.poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(crate::TimeoutError::from(this.duration))),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(crate) async fn yield_now() {
    YieldNow { yielded: false }.await
}

struct YieldNow {
    yielded: bool,
}

impl Future for YieldNow {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if !this.yielded {
            this.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

#[derive(Debug)]
pub(crate) struct InHouseJoinHandle<T> {
    receiver: Mutex<Option<oneshot::Receiver<Result<T, InHouseJoinError>>>>,
    sender: JoinSenderSlot<T>,
    cancelled: Arc<AtomicBool>,
    task: Option<Weak<Task>>,
}

impl<T> InHouseJoinHandle<T> {
    fn new(
        receiver: oneshot::Receiver<Result<T, InHouseJoinError>>,
        sender: JoinSenderSlot<T>,
        cancelled: Arc<AtomicBool>,
        task: Option<Arc<Task>>,
    ) -> Self {
        Self {
            receiver: Mutex::new(Some(receiver)),
            sender,
            cancelled,
            task: task.map(|task| Arc::downgrade(&task)),
        }
    }

    pub(crate) fn abort(&self) {
        if !self.cancelled.swap(true, AtomicOrdering::SeqCst) {
            if let Some(task) = self.task.as_ref().and_then(|task| task.upgrade()) {
                task.schedule();
            }
            if let Some(sender) = self.sender.lock().recover().take() {
                let _ = sender.send(Err(InHouseJoinError::cancelled()));
            }
        }
    }

    pub(crate) fn poll(&self, cx: &mut Context<'_>) -> Poll<Result<T, InHouseJoinError>> {
        let mut receiver = self.receiver.lock().recover();
        let receiver = receiver
            .as_mut()
            .unwrap_or_else(|| panic!("inhouse join handle missing receiver"));
        match Pin::new(receiver).poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(InHouseJoinError::cancelled())),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Debug)]
pub(crate) struct InHouseJoinError {
    kind: InHouseJoinErrorKind,
}

#[derive(Debug, Clone, Copy)]
enum InHouseJoinErrorKind {
    Cancelled,
    Panic,
}

impl InHouseJoinError {
    fn cancelled() -> Self {
        Self {
            kind: InHouseJoinErrorKind::Cancelled,
        }
    }

    fn panic() -> Self {
        Self {
            kind: InHouseJoinErrorKind::Panic,
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        matches!(self.kind, InHouseJoinErrorKind::Cancelled)
    }

    pub(crate) fn is_panic(&self) -> bool {
        matches!(self.kind, InHouseJoinErrorKind::Panic)
    }
}

impl fmt::Display for InHouseJoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            InHouseJoinErrorKind::Cancelled => write!(f, "in-house runtime task aborted"),
            InHouseJoinErrorKind::Panic => write!(f, "in-house runtime task panicked"),
        }
    }
}

impl std::error::Error for InHouseJoinError {}

struct CancelableFuture<F> {
    inner: Pin<Box<F>>,
    cancelled: Arc<AtomicBool>,
}

impl<F> CancelableFuture<F> {
    fn new(inner: F, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            inner: Box::pin(inner),
            cancelled,
        }
    }
}

enum CancelOutcome<T> {
    Completed(T),
    Cancelled,
}

impl<F> Future for CancelableFuture<F>
where
    F: Future,
{
    type Output = CancelOutcome<F::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.cancelled.load(AtomicOrdering::SeqCst) {
            return Poll::Ready(CancelOutcome::Cancelled);
        }

        match this.inner.as_mut().poll(cx) {
            Poll::Ready(value) => Poll::Ready(CancelOutcome::Completed(value)),
            Poll::Pending => Poll::Pending,
        }
    }
}

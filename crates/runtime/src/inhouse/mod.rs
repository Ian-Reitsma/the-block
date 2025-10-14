use foundation_async::future::catch_unwind;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::fmt;
use std::future::Future;
use std::panic::{self, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use crate::sync::oneshot;
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use foundation_async::block_on;
use mio::{event::Source, Events, Interest, Poll as MioPoll, Token, Waker as MioWaker};
use std::io;

pub(crate) mod net;

const SPAWN_LATENCY_METRIC: &str = "runtime_spawn_latency_seconds";
const PENDING_TASKS_METRIC: &str = "runtime_pending_tasks";
const REACTOR_WAKER_TOKEN: Token = Token(usize::MAX - 1);

pub(crate) struct InHouseRuntime {
    inner: Arc<Inner>,
}

type PendingCounter = Arc<AtomicI64>;

type TaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

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
                    if let Some(sender) = sender_for_task
                        .lock()
                        .expect("inhouse sender poisoned")
                        .take()
                    {
                        let _ = sender.send(Ok(value));
                    }
                }
                Ok(CancelOutcome::Cancelled) => {
                    if let Some(sender) = sender_for_task
                        .lock()
                        .expect("inhouse sender poisoned")
                        .take()
                    {
                        let _ = sender.send(Err(InHouseJoinError::cancelled()));
                    }
                }
                Err(_) => {
                    if let Some(sender) = sender_for_task
                        .lock()
                        .expect("inhouse sender poisoned")
                        .take()
                    {
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
                if let Some(sender) = sender_for_job
                    .lock()
                    .expect("inhouse sender poisoned")
                    .take()
                {
                    let _ = sender.send(Err(InHouseJoinError::cancelled()));
                }
                return;
            }

            match panic::catch_unwind(AssertUnwindSafe(func)) {
                Ok(value) => {
                    if let Some(sender) = sender_for_job
                        .lock()
                        .expect("inhouse sender poisoned")
                        .take()
                    {
                        let _ = sender.send(Ok(value));
                    }
                }
                Err(_) => {
                    if let Some(sender) = sender_for_job
                        .lock()
                        .expect("inhouse sender poisoned")
                        .take()
                    {
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
    injector: Arc<Injector<Arc<Task>>>,
    notify: WorkerNotify,
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
        let injector = Arc::new(Injector::new());
        let mut workers = Vec::with_capacity(worker_count);
        let mut stealers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let worker = Worker::new_fifo();
            stealers.push(worker.stealer());
            workers.push(worker);
        }
        let stealers = Arc::new(stealers);
        let reactor = ReactorInner::new();
        let inner = Arc::new(Self {
            injector: Arc::clone(&injector),
            notify: WorkerNotify::new(),
            shutdown: AtomicBool::new(false),
            pending: Arc::new(AtomicI64::new(0)),
            reactor,
            blocking: BlockingPool::new(worker_count.max(2)),
            worker_handles: Mutex::new(Vec::new()),
        });
        inner.spawn_workers(workers, injector, stealers);
        inner
    }

    fn spawn_workers(
        self: &Arc<Self>,
        workers: Vec<Worker<Arc<Task>>>,
        injector: Arc<Injector<Arc<Task>>>,
        stealers: Arc<Vec<Stealer<Arc<Task>>>>,
    ) {
        let mut handles = self
            .worker_handles
            .lock()
            .expect("worker handle mutex poisoned");
        for (index, worker) in workers.into_iter().enumerate() {
            let runtime = Arc::clone(self);
            let injector_clone = Arc::clone(&injector);
            let stealers_clone = Arc::clone(&stealers);
            let handle = thread::Builder::new()
                .name(format!("inhouse-runtime-worker-{index}"))
                .spawn(move || {
                    SchedulerWorker::new(runtime, worker, injector_clone, stealers_clone, index)
                        .run();
                })
                .expect("failed to spawn in-house runtime worker");
            handles.push(handle);
        }
    }

    fn schedule(&self, task: Arc<Task>) {
        self.injector.push(task);
        self.notify.notify_one();
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.shutdown.store(true, AtomicOrdering::SeqCst);
        self.notify.notify_all();
        self.reactor.shutdown();
        self.blocking.shutdown();
        let mut handles = self
            .worker_handles
            .lock()
            .expect("worker handle mutex poisoned");
        for handle in handles.drain(..) {
            let _ = handle.join();
        }
    }
}

struct SchedulerWorker {
    runtime: Arc<Inner>,
    local: Worker<Arc<Task>>,
    injector: Arc<Injector<Arc<Task>>>,
    stealers: Arc<Vec<Stealer<Arc<Task>>>>,
    index: usize,
}

impl SchedulerWorker {
    fn new(
        runtime: Arc<Inner>,
        local: Worker<Arc<Task>>,
        injector: Arc<Injector<Arc<Task>>>,
        stealers: Arc<Vec<Stealer<Arc<Task>>>>,
        index: usize,
    ) -> Self {
        Self {
            runtime,
            local,
            injector,
            stealers,
            index,
        }
    }

    fn run(mut self) {
        loop {
            if self.runtime.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }

            if let Some(task) = self.pop_task() {
                task.run();
                continue;
            }

            if self.runtime.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }

            self.runtime.notify.wait();
        }
    }

    fn pop_task(&mut self) -> Option<Arc<Task>> {
        if let Some(task) = self.local.pop() {
            return Some(task);
        }

        if let Steal::Success(task) = self.injector.steal_batch_and_pop(&self.local) {
            return Some(task);
        }

        for (idx, stealer) in self.stealers.iter().enumerate() {
            if idx == self.index {
                continue;
            }
            match stealer.steal() {
                Steal::Success(task) => return Some(task),
                Steal::Retry => return self.pop_task(),
                Steal::Empty => continue,
            }
        }
        None
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
        let mut slot = self.future.lock().expect("inhouse task mutex poisoned");
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

#[derive(Clone)]
struct WorkerNotify {
    inner: Arc<WorkerNotifyInner>,
}

struct WorkerNotifyInner {
    state: Mutex<usize>,
    cv: Condvar,
}

impl WorkerNotify {
    fn new() -> Self {
        Self {
            inner: Arc::new(WorkerNotifyInner {
                state: Mutex::new(0),
                cv: Condvar::new(),
            }),
        }
    }

    fn notify_one(&self) {
        let mut guard = self
            .inner
            .state
            .lock()
            .expect("worker notify mutex poisoned");
        if *guard != usize::MAX {
            *guard = guard.saturating_add(1);
        }
        self.inner.cv.notify_one();
    }

    fn notify_all(&self) {
        let mut guard = self
            .inner
            .state
            .lock()
            .expect("worker notify mutex poisoned");
        *guard = usize::MAX;
        self.inner.cv.notify_all();
    }

    fn wait(&self) {
        let mut guard = self
            .inner
            .state
            .lock()
            .expect("worker notify mutex poisoned");
        while *guard == 0 {
            guard = self
                .inner
                .cv
                .wait(guard)
                .expect("worker notify wait poisoned");
        }
        if *guard != usize::MAX {
            *guard -= 1;
        }
    }
}

struct BlockingPool {
    injector: Arc<Injector<BlockingJob>>,
    notify: WorkerNotify,
    shutdown: Arc<AtomicBool>,
    workers: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl BlockingPool {
    fn new(worker_count: usize) -> Self {
        let injector = Arc::new(Injector::new());
        let mut workers = Vec::with_capacity(worker_count);
        let mut stealers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let worker = Worker::new_fifo();
            stealers.push(worker.stealer());
            workers.push(worker);
        }
        let stealers_arc = Arc::new(stealers);
        let notify = WorkerNotify::new();
        let shutdown = Arc::new(AtomicBool::new(false));
        let handles = workers
            .into_iter()
            .enumerate()
            .map(|(index, worker)| {
                let injector_clone = Arc::clone(&injector);
                let stealers_clone = Arc::clone(&stealers_arc);
                let notify_clone = notify.clone();
                let shutdown_clone = Arc::clone(&shutdown);
                thread::Builder::new()
                    .name(format!("inhouse-blocking-worker-{index}"))
                    .spawn(move || {
                        BlockingWorker::new(
                            worker,
                            injector_clone,
                            stealers_clone,
                            notify_clone,
                            shutdown_clone,
                            index,
                        )
                        .run();
                    })
                    .expect("failed to spawn in-house blocking worker")
            })
            .collect();
        Self {
            injector,
            notify,
            shutdown,
            workers: Mutex::new(handles),
        }
    }

    fn spawn(&self, job: BlockingJob) {
        self.injector.push(job);
        self.notify.notify_one();
    }

    fn shutdown(&self) {
        if self.shutdown.swap(true, AtomicOrdering::SeqCst) {
            return;
        }
        self.notify.notify_all();
        let mut handles = self.workers.lock().expect("blocking worker mutex poisoned");
        for handle in handles.drain(..) {
            let _ = handle.join();
        }
    }
}

struct BlockingWorker {
    local: Worker<BlockingJob>,
    injector: Arc<Injector<BlockingJob>>,
    stealers: Arc<Vec<Stealer<BlockingJob>>>,
    notify: WorkerNotify,
    shutdown: Arc<AtomicBool>,
    index: usize,
}

impl BlockingWorker {
    fn new(
        local: Worker<BlockingJob>,
        injector: Arc<Injector<BlockingJob>>,
        stealers: Arc<Vec<Stealer<BlockingJob>>>,
        notify: WorkerNotify,
        shutdown: Arc<AtomicBool>,
        index: usize,
    ) -> Self {
        Self {
            local,
            injector,
            stealers,
            notify,
            shutdown,
            index,
        }
    }

    fn run(mut self) {
        loop {
            if self.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }

            if let Some(job) = self.pop_job() {
                job.run();
                continue;
            }

            if self.shutdown.load(AtomicOrdering::SeqCst) {
                break;
            }

            self.notify.wait();
        }
    }

    fn pop_job(&mut self) -> Option<BlockingJob> {
        if let Some(job) = self.local.pop() {
            return Some(job);
        }

        if let Steal::Success(job) = self.injector.steal_batch_and_pop(&self.local) {
            return Some(job);
        }

        for (idx, stealer) in self.stealers.iter().enumerate() {
            if idx == self.index {
                continue;
            }
            match stealer.steal() {
                Steal::Success(job) => return Some(job),
                Steal::Retry => return self.pop_job(),
                Steal::Empty => continue,
            }
        }
        None
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
    poll: Mutex<MioPoll>,
    waker: MioWaker,
    timers: Mutex<TimerState>,
    io: Mutex<IoState>,
    next_token: AtomicU64,
    shutdown: AtomicBool,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl ReactorInner {
    fn new() -> Arc<Self> {
        let poll = MioPoll::new().expect("failed to create reactor poller");
        let waker = MioWaker::new(poll.registry(), REACTOR_WAKER_TOKEN)
            .expect("failed to create reactor waker");
        let inner = Arc::new(Self {
            poll: Mutex::new(poll),
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
            .expect("failed to spawn in-house reactor");
        *inner.thread.lock().expect("reactor thread mutex poisoned") = Some(handle);
        inner
    }

    fn run(self: Arc<Self>) {
        let mut events = Events::with_capacity(128);
        while !self.shutdown.load(AtomicOrdering::SeqCst) {
            let timeout = self.compute_timeout();
            {
                let mut poll = self.poll.lock().expect("reactor poll mutex poisoned");
                let _ = poll.poll(&mut events, timeout);
            }
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
        let mut state = self.timers.lock().expect("reactor timers mutex poisoned");
        state.prune();
        state
            .peek_deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    fn fire_due_timers(&self) {
        loop {
            let maybe_waker = {
                let mut state = self.timers.lock().expect("reactor timers mutex poisoned");
                state.pop_due()
            };
            match maybe_waker {
                Some(waker) => waker.wake(),
                None => break,
            }
        }
    }

    fn dispatch_io_event(&self, event: &mio::event::Event) {
        let token_index = event.token().0;
        let mut wake_read = None;
        let mut wake_write = None;

        {
            let mut state = self.io.lock().expect("reactor io mutex poisoned");
            if let Some(entry) = state.entries.get_mut(&token_index) {
                if event.is_readable()
                    || event.is_read_closed()
                    || event.is_error()
                    || event.is_priority()
                {
                    entry.read_ready = true;
                    wake_read = entry.read_waker.take();
                }
                if event.is_writable()
                    || event.is_write_closed()
                    || event.is_error()
                    || event.is_priority()
                {
                    entry.write_ready = true;
                    wake_write = entry.write_waker.take();
                }
            }
        }

        if let Some(waker) = wake_read {
            waker.wake();
        }
        if let Some(waker) = wake_write {
            waker.wake();
        }
    }

    fn register_source(&self, source: &mut impl Source, interest: Interest) -> io::Result<Token> {
        let token_value = self.next_token.fetch_add(1, AtomicOrdering::SeqCst) as usize;
        let token = Token(token_value);
        let _ = self.waker.wake();
        let poll = loop {
            if let Ok(poll) = self.poll.try_lock() {
                break poll;
            }
            let _ = self.waker.wake();
            thread::yield_now();
        };
        poll.registry().register(source, token, interest)?;
        {
            let mut state = self.io.lock().expect("reactor io mutex poisoned");
            state.insert(token);
        }
        Ok(token)
    }

    fn deregister_source(&self, source: &mut impl Source, token: Token) -> io::Result<()> {
        let _ = self.waker.wake();
        let poll = loop {
            if let Ok(poll) = self.poll.try_lock() {
                break poll;
            }
            let _ = self.waker.wake();
            thread::yield_now();
        };
        poll.registry().deregister(source)?;
        let mut state = self.io.lock().expect("reactor io mutex poisoned");
        state.remove(token);
        Ok(())
    }

    fn poll_io_ready(
        &self,
        token: Token,
        direction: IoDirection,
        cx: &Context<'_>,
    ) -> Poll<io::Result<()>> {
        {
            let mut state = self.io.lock().expect("reactor io mutex poisoned");
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

        Poll::Pending
    }

    fn poll_read_ready(&self, token: Token, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.poll_io_ready(token, IoDirection::Read, cx)
    }

    fn poll_write_ready(&self, token: Token, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.poll_io_ready(token, IoDirection::Write, cx)
    }

    fn register_timer(
        self: &Arc<Self>,
        deadline: Instant,
        waker: Waker,
        interval: Option<Duration>,
    ) -> TimerRegistration {
        let id = self.next_token.fetch_add(1, AtomicOrdering::SeqCst);
        {
            let mut state = self.timers.lock().expect("reactor timers mutex poisoned");
            state.insert(id, deadline, waker, interval);
        }
        let _ = self.waker.wake();
        TimerRegistration {
            id,
            reactor: Arc::clone(self),
        }
    }

    fn update_waker(&self, id: u64, waker: Waker) {
        let mut state = self.timers.lock().expect("reactor timers mutex poisoned");
        state.update_waker(id, waker);
    }

    fn cancel(&self, id: u64) {
        let mut state = self.timers.lock().expect("reactor timers mutex poisoned");
        state.cancel(id);
    }

    fn shutdown(&self) {
        if self.shutdown.swap(true, AtomicOrdering::SeqCst) {
            return;
        }
        let _ = self.waker.wake();
        if let Some(handle) = self
            .thread
            .lock()
            .expect("reactor thread mutex poisoned")
            .take()
        {
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
}

impl IoState {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn insert(&mut self, token: Token) {
        self.entries.insert(token.0, IoEntry::new());
    }

    fn remove(&mut self, token: Token) {
        self.entries.remove(&token.0);
    }
}

struct IoEntry {
    read_ready: bool,
    write_ready: bool,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
}

impl IoEntry {
    fn new() -> Self {
        Self {
            read_ready: false,
            write_ready: false,
            read_waker: None,
            write_waker: None,
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
}

impl IoRegistration {
    pub(crate) fn new(
        reactor: Arc<ReactorInner>,
        source: &mut impl Source,
        interest: Interest,
    ) -> io::Result<Self> {
        let token = reactor.register_source(source, interest)?;
        Ok(Self { reactor, token })
    }

    pub(crate) fn poll_read_ready(&self, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.reactor.poll_read_ready(self.token, cx)
    }

    pub(crate) fn poll_write_ready(&self, cx: &Context<'_>) -> Poll<io::Result<()>> {
        self.reactor.poll_write_ready(self.token, cx)
    }

    pub(crate) fn deregister(&self, source: &mut impl Source) -> io::Result<()> {
        self.reactor.deregister_source(source, self.token)
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

impl<'a> Future for IntervalTick<'a> {
    type Output = Instant;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if Instant::now() >= this.interval.next {
            let fired = this.interval.next;
            this.interval.next = this.interval.next + this.interval.period;
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
    sender: Arc<Mutex<Option<oneshot::Sender<Result<T, InHouseJoinError>>>>>,
    cancelled: Arc<AtomicBool>,
    task: Option<Weak<Task>>,
}

impl<T> InHouseJoinHandle<T> {
    fn new(
        receiver: oneshot::Receiver<Result<T, InHouseJoinError>>,
        sender: Arc<Mutex<Option<oneshot::Sender<Result<T, InHouseJoinError>>>>>,
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
            if let Some(sender) = self.sender.lock().expect("inhouse sender poisoned").take() {
                let _ = sender.send(Err(InHouseJoinError::cancelled()));
            }
        }
    }

    pub(crate) fn poll(&self, cx: &mut Context<'_>) -> Poll<Result<T, InHouseJoinError>> {
        let mut receiver = self
            .receiver
            .lock()
            .expect("inhouse join handle mutex poisoned");
        let receiver = receiver
            .as_mut()
            .expect("inhouse join handle missing receiver");
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

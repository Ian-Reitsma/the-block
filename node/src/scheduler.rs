use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use crate::utxo::{OutPoint, Transaction};

#[cfg(feature = "telemetry")]
use crate::telemetry::SCHEDULER_CLASS_WAIT_SECONDS;

static DEFAULT_GOSSIP_WEIGHT: AtomicU32 = AtomicU32::new(3);
static DEFAULT_COMPUTE_WEIGHT: AtomicU32 = AtomicU32::new(2);
static DEFAULT_STORAGE_WEIGHT: AtomicU32 = AtomicU32::new(1);

static GLOBAL_STATS: Lazy<RwLock<ServiceSchedulerStats>> = Lazy::new(|| {
    RwLock::new(ServiceSchedulerStats {
        reentrant_enabled: cfg!(feature = "reentrant_scheduler"),
        ..ServiceSchedulerStats::default()
    })
});

/// Scheduler priority classes used for proof-of-service tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ServiceClass {
    Gossip,
    Compute,
    Storage,
}

impl ServiceClass {
    pub const ALL: [ServiceClass; 3] = [
        ServiceClass::Gossip,
        ServiceClass::Compute,
        ServiceClass::Storage,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ServiceClass::Gossip => "gossip",
            ServiceClass::Compute => "compute",
            ServiceClass::Storage => "storage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceWeights {
    pub gossip: u32,
    pub compute: u32,
    pub storage: u32,
}

impl ServiceWeights {
    pub fn new(gossip: u32, compute: u32, storage: u32) -> Self {
        Self {
            gossip,
            compute,
            storage,
        }
    }

    pub fn weight(&self, class: ServiceClass) -> u32 {
        match class {
            ServiceClass::Gossip => self.gossip,
            ServiceClass::Compute => self.compute,
            ServiceClass::Storage => self.storage,
        }
    }

    pub fn as_map(&self) -> HashMap<&'static str, u32> {
        let mut map = HashMap::new();
        map.insert(ServiceClass::Gossip.as_str(), self.gossip);
        map.insert(ServiceClass::Compute.as_str(), self.compute);
        map.insert(ServiceClass::Storage.as_str(), self.storage);
        map
    }
}

impl Default for ServiceWeights {
    fn default() -> Self {
        current_default_weights()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServiceSchedulerStats {
    pub queue_depths: HashMap<ServiceClass, usize>,
    pub weights: ServiceWeights,
    pub reentrant_enabled: bool,
}

impl ServiceSchedulerStats {
    fn record_queue_depths<T>(scheduler: &ServiceScheduler<T>) -> Self {
        let mut depths = HashMap::new();
        for class in ServiceClass::ALL.iter() {
            depths.insert(*class, scheduler.queue_len(*class));
        }
        Self {
            queue_depths: depths,
            weights: scheduler.weights,
            reentrant_enabled: scheduler.reentrant_enabled(),
        }
    }
}

#[derive(Debug)]
struct QueuedTask<T> {
    payload: T,
    enqueued: Instant,
}

#[derive(Debug, Clone)]
pub struct ScheduledTask<T> {
    pub class: ServiceClass,
    pub payload: T,
    pub wait: Duration,
}

impl<T> ScheduledTask<T> {
    pub fn into_inner(self) -> T {
        self.payload
    }
}

pub struct ServiceScheduler<T> {
    queues: HashMap<ServiceClass, VecDeque<QueuedTask<T>>>,
    weights: ServiceWeights,
    #[cfg(feature = "reentrant_scheduler")]
    current: Option<ServiceClass>,
    #[cfg(feature = "reentrant_scheduler")]
    budget: u32,
    #[cfg(feature = "reentrant_scheduler")]
    last_idx: usize,
    #[cfg(not(feature = "reentrant_scheduler"))]
    order: VecDeque<ServiceClass>,
}

impl<T> ServiceScheduler<T> {
    pub fn new(weights: ServiceWeights) -> Self {
        let mut queues = HashMap::new();
        for class in ServiceClass::ALL.iter() {
            queues.insert(*class, VecDeque::new());
        }
        let mut sched = Self {
            queues,
            weights,
            #[cfg(feature = "reentrant_scheduler")]
            current: None,
            #[cfg(feature = "reentrant_scheduler")]
            budget: 0,
            #[cfg(feature = "reentrant_scheduler")]
            last_idx: 0,
            #[cfg(not(feature = "reentrant_scheduler"))]
            order: VecDeque::new(),
        };
        sched.update_stats();
        sched
    }

    pub fn with_defaults() -> Self {
        Self::new(ServiceWeights::default())
    }

    pub fn enqueue(&mut self, class: ServiceClass, payload: T) {
        if let Some(queue) = self.queues.get_mut(&class) {
            queue.push_back(QueuedTask {
                payload,
                enqueued: Instant::now(),
            });
        }
        #[cfg(not(feature = "reentrant_scheduler"))]
        self.order.push_back(class);
        self.update_stats();
    }

    pub fn dequeue(&mut self) -> Option<ScheduledTask<T>> {
        #[cfg(feature = "reentrant_scheduler")]
        {
            let class = self.pick_class()?;
            let queue = self.queues.get_mut(&class)?;
            if let Some(task) = queue.pop_front() {
                let wait = task.enqueued.elapsed();
                if queue.is_empty() {
                    self.budget = 0;
                }
                #[cfg(feature = "telemetry")]
                SCHEDULER_CLASS_WAIT_SECONDS
                    .with_label_values(&[class.as_str()])
                    .observe(wait.as_secs_f64());
                let scheduled = ScheduledTask {
                    class,
                    payload: task.payload,
                    wait,
                };
                self.update_stats();
                return Some(scheduled);
            }
            self.update_stats();
            None
        }
        #[cfg(not(feature = "reentrant_scheduler"))]
        {
            while let Some(class) = self.order.pop_front() {
                if let Some(queue) = self.queues.get_mut(&class) {
                    if let Some(task) = queue.pop_front() {
                        let wait = task.enqueued.elapsed();
                        #[cfg(feature = "telemetry")]
                        SCHEDULER_CLASS_WAIT_SECONDS
                            .with_label_values(&[class.as_str()])
                            .observe(wait.as_secs_f64());
                        let scheduled = ScheduledTask {
                            class,
                            payload: task.payload,
                            wait,
                        };
                        self.update_stats();
                        return Some(scheduled);
                    }
                }
            }
            self.update_stats();
            None
        }
    }

    pub fn drain(&mut self, limit: usize) -> Vec<ScheduledTask<T>> {
        let mut out = Vec::new();
        for _ in 0..limit {
            if let Some(task) = self.dequeue() {
                out.push(task);
            } else {
                break;
            }
        }
        out
    }

    pub fn set_weights(&mut self, weights: ServiceWeights) {
        self.weights = weights;
        #[cfg(feature = "reentrant_scheduler")]
        {
            self.current = None;
            self.budget = 0;
        }
        self.update_stats();
    }

    pub fn stats(&self) -> ServiceSchedulerStats {
        ServiceSchedulerStats::record_queue_depths(self)
    }

    pub fn reentrant_enabled(&self) -> bool {
        cfg!(feature = "reentrant_scheduler")
    }

    fn queue_len(&self, class: ServiceClass) -> usize {
        self.queues.get(&class).map(|q| q.len()).unwrap_or(0)
    }

    fn update_stats(&self) {
        if let Ok(mut guard) = GLOBAL_STATS.write() {
            *guard = ServiceSchedulerStats::record_queue_depths(self);
        }
    }

    #[cfg(feature = "reentrant_scheduler")]
    fn pick_class(&mut self) -> Option<ServiceClass> {
        if let Some(current) = self.current {
            if self.budget > 0 {
                if let Some(queue) = self.queues.get(&current) {
                    if !queue.is_empty() {
                        self.budget -= 1;
                        return Some(current);
                    }
                }
            }
        }
        let classes = ServiceClass::ALL;
        for _ in 0..classes.len() {
            self.last_idx = (self.last_idx + 1) % classes.len();
            let candidate = classes[self.last_idx];
            let weight = self.weights.weight(candidate);
            if weight == 0 {
                continue;
            }
            if let Some(queue) = self.queues.get(&candidate) {
                if !queue.is_empty() {
                    self.current = Some(candidate);
                    self.budget = weight.saturating_sub(1);
                    return Some(candidate);
                }
            }
        }
        self.current = None;
        None
    }
}

impl<T> Default for ServiceScheduler<T> {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl<T: Send + Sync> ServiceScheduler<crate::parallel::Task<T>> {
    pub fn execute_ready(&mut self, limit: usize) -> Vec<T> {
        let tasks: Vec<_> = self
            .drain(limit)
            .into_iter()
            .map(|scheduled| scheduled.payload)
            .collect();
        crate::parallel::ParallelExecutor::execute(tasks)
    }
}

pub fn set_default_weights(gossip: u32, compute: u32, storage: u32) {
    DEFAULT_GOSSIP_WEIGHT.store(gossip, Ordering::Relaxed);
    DEFAULT_COMPUTE_WEIGHT.store(compute, Ordering::Relaxed);
    DEFAULT_STORAGE_WEIGHT.store(storage, Ordering::Relaxed);
}

pub fn set_weight(class: ServiceClass, weight: u32) {
    match class {
        ServiceClass::Gossip => DEFAULT_GOSSIP_WEIGHT.store(weight, Ordering::Relaxed),
        ServiceClass::Compute => DEFAULT_COMPUTE_WEIGHT.store(weight, Ordering::Relaxed),
        ServiceClass::Storage => DEFAULT_STORAGE_WEIGHT.store(weight, Ordering::Relaxed),
    }
}

pub fn current_default_weights() -> ServiceWeights {
    ServiceWeights {
        gossip: DEFAULT_GOSSIP_WEIGHT.load(Ordering::Relaxed),
        compute: DEFAULT_COMPUTE_WEIGHT.load(Ordering::Relaxed),
        storage: DEFAULT_STORAGE_WEIGHT.load(Ordering::Relaxed),
    }
}

pub fn global_stats_snapshot() -> ServiceSchedulerStats {
    GLOBAL_STATS
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

#[derive(Default)]
pub struct TxScheduler {
    running: HashMap<[u8; 32], TxRwSet>,
}

#[derive(Clone)]
struct TxRwSet {
    reads: HashSet<OutPoint>,
    writes: HashSet<OutPoint>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScheduleError {
    Conflict([u8; 32]),
}

impl TxScheduler {
    pub fn schedule(&mut self, tx: &Transaction) -> Result<(), ScheduleError> {
        let txid = tx.txid();
        let ours = TxRwSet::from_tx(tx);
        for (other_id, other) in &self.running {
            if ours.conflicts(other) {
                return Err(ScheduleError::Conflict(*other_id));
            }
        }
        self.running.insert(txid, ours);
        Ok(())
    }

    pub fn complete(&mut self, tx: &Transaction) {
        self.running.remove(&tx.txid());
    }
}

impl TxRwSet {
    fn from_tx(tx: &Transaction) -> Self {
        let reads = tx
            .inputs
            .iter()
            .map(|i| i.previous_output.clone())
            .collect();
        let writes = tx
            .outputs
            .iter()
            .enumerate()
            .map(|(i, _)| OutPoint {
                txid: tx.txid(),
                index: i as u32,
            })
            .collect();
        Self { reads, writes }
    }

    fn conflicts(&self, other: &TxRwSet) -> bool {
        !self.reads.is_disjoint(&other.writes)
            || !self.writes.is_disjoint(&other.reads)
            || !self.writes.is_disjoint(&other.writes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utxo::{Script, Transaction};
    use crate::utxo::{TxIn, TxOut};

    fn mk_tx(read: Option<OutPoint>, _write_idx: u32) -> Transaction {
        let inputs = read
            .into_iter()
            .map(|previous_output| TxIn {
                previous_output,
                script_sig: Script(vec![]),
            })
            .collect();
        Transaction {
            inputs,
            outputs: vec![TxOut {
                value: 1,
                script_pubkey: Script(vec![]),
            }],
        }
    }

    #[test]
    fn detects_conflict() {
        let mut sched = TxScheduler::default();
        let op = OutPoint {
            txid: [1; 32],
            index: 0,
        };
        let tx1 = mk_tx(Some(op.clone()), 0);
        let tx2 = mk_tx(Some(op), 1);
        assert!(sched.schedule(&tx1).is_ok());
        assert_eq!(
            sched.schedule(&tx2),
            Err(ScheduleError::Conflict(tx1.txid()))
        );
    }
}

use crate::{PeerId, UptimeHandle, UptimeInfo, UptimeMetrics, UptimeStore};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct InMemoryUptimeStore<P>
where
    P: PeerId,
{
    inner: Mutex<HashMap<P, UptimeInfo>>,
}

impl<P> InMemoryUptimeStore<P>
where
    P: PeerId,
{
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl<P> UptimeStore<P> for InMemoryUptimeStore<P>
where
    P: PeerId,
{
    fn with_map<R>(&self, f: impl FnOnce(&mut HashMap<P, UptimeInfo>) -> R) -> R {
        let mut guard = self.inner.lock().expect("uptime map poisoned");
        f(&mut guard)
    }
}

pub struct UptimeTracker<P, S, M = crate::NoopMetrics>
where
    P: PeerId,
    S: UptimeStore<P>,
    M: UptimeMetrics,
{
    store: S,
    metrics: M,
    _marker: PhantomData<fn() -> P>,
}

impl<P, S> UptimeTracker<P, S>
where
    P: PeerId,
    S: UptimeStore<P>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            metrics: crate::NoopMetrics,
            _marker: PhantomData,
        }
    }
}

impl<P, S, M> UptimeTracker<P, S, M>
where
    P: PeerId,
    S: UptimeStore<P>,
    M: UptimeMetrics,
{
    pub fn with_metrics(store: S, metrics: M) -> Self {
        Self {
            store,
            metrics,
            _marker: PhantomData,
        }
    }

    pub fn note_seen(&self, peer: P) {
        self.store.with_map(|map| {
            let entry = map.entry(peer).or_default();
            let now = current_secs();
            if entry.last > 0 {
                entry.total = entry.total.saturating_add(now - entry.last);
            }
            entry.last = now;
        });
    }

    pub fn eligible(&self, peer: &P, threshold: u64, epoch: u64) -> bool {
        self.store.with_map(|map| {
            map.get(peer)
                .map(|info| info.total >= threshold && info.claimed_epoch < epoch)
                .unwrap_or(false)
        })
    }

    pub fn claim(&self, peer: P, threshold: u64, epoch: u64, reward: u64) -> Option<u64> {
        let issued = self.store.with_map(|map| {
            let info = map.entry(peer).or_default();
            if info.total >= threshold && info.claimed_epoch < epoch {
                info.claimed_epoch = epoch;
                true
            } else {
                false
            }
        });
        if issued {
            self.metrics.on_claim();
            self.metrics.on_issue();
            Some(reward)
        } else {
            None
        }
    }

    pub fn tracked_peers(&self) -> usize {
        self.store.with_map(|map| map.len())
    }

    pub fn handle(self: &Arc<Self>) -> Arc<dyn UptimeHandle<Peer = P>>
    where
        P: 'static,
        S: 'static,
        M: 'static,
    {
        self.clone()
    }
}

fn current_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl<P, S, M> UptimeHandle for UptimeTracker<P, S, M>
where
    P: PeerId,
    S: UptimeStore<P>,
    M: UptimeMetrics,
{
    type Peer = P;

    fn note_seen(&self, peer: Self::Peer) {
        UptimeTracker::note_seen(self, peer);
    }

    fn eligible(&self, peer: &Self::Peer, threshold: u64, epoch: u64) -> bool {
        UptimeTracker::eligible(self, peer, threshold, epoch)
    }

    fn claim(&self, peer: Self::Peer, threshold: u64, epoch: u64, reward: u64) -> Option<u64> {
        UptimeTracker::claim(self, peer, threshold, epoch, reward)
    }
}

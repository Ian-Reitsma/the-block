use crate::compute_market::courier_store::ReceiptStore;
use crate::compute_market::receipt::Receipt;
use crate::transaction::FeeLane;
use concurrency::{DashMap, Lazy};
use runtime::sync::CancellationToken;
use runtime::yield_now;
use std::collections::VecDeque;
use std::env;
use std::fmt;
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
pub struct Bid {
    pub job_id: String,
    pub buyer: String,
    pub price: u64,
    pub lane: FeeLane,
}

#[derive(Clone)]
pub struct Ask {
    pub job_id: String,
    pub provider: String,
    pub price: u64,
    pub lane: FeeLane,
}

#[derive(Clone)]
pub struct LaneMetadata {
    pub fairness_window: Duration,
    pub max_queue_depth: usize,
}

impl Default for LaneMetadata {
    fn default() -> Self {
        Self {
            fairness_window: default_fairness_window(),
            max_queue_depth: default_lane_capacity(),
        }
    }
}

#[derive(Clone)]
pub struct LaneSeed {
    pub lane: FeeLane,
    pub bids: Vec<Bid>,
    pub asks: Vec<Ask>,
    pub metadata: LaneMetadata,
}

#[derive(Debug)]
pub enum SeedError {
    CapacityExceeded {
        lane: FeeLane,
        attempted: usize,
        max: usize,
    },
}

impl fmt::Display for SeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeedError::CapacityExceeded {
                lane,
                attempted,
                max,
            } => write!(
                f,
                "lane {lane} exceeds capacity {max} (attempted {attempted})"
            ),
        }
    }
}

impl std::error::Error for SeedError {}

#[derive(Clone)]
pub struct LaneStatus {
    pub lane: FeeLane,
    pub bids: usize,
    pub asks: usize,
    pub oldest_bid_wait: Option<Duration>,
    pub oldest_ask_wait: Option<Duration>,
}

#[derive(Clone)]
pub struct LaneWarning {
    pub lane: FeeLane,
    pub oldest_job: String,
    pub waited_for: Duration,
    pub updated_at: SystemTime,
}

#[derive(Clone)]
pub struct LaneSnapshot {
    pub lane: FeeLane,
    pub bids: Vec<Bid>,
    pub asks: Vec<Ask>,
}

struct QueuedBid {
    bid: Bid,
    enqueued_at: Instant,
    _enqueued_wallclock: SystemTime,
}

struct QueuedAsk {
    ask: Ask,
    enqueued_at: Instant,
    _enqueued_wallclock: SystemTime,
}

impl QueuedBid {
    fn new(bid: Bid) -> Self {
        Self {
            bid,
            enqueued_at: Instant::now(),
            _enqueued_wallclock: SystemTime::now(),
        }
    }
}

impl QueuedAsk {
    fn new(ask: Ask) -> Self {
        Self {
            ask,
            enqueued_at: Instant::now(),
            _enqueued_wallclock: SystemTime::now(),
        }
    }
}

struct LaneState {
    bids: VecDeque<QueuedBid>,
    asks: VecDeque<QueuedAsk>,
    metadata: LaneMetadata,
    last_match_at: Option<Instant>,
    last_warning_at: Option<Instant>,
}

impl LaneState {
    fn new(metadata: LaneMetadata) -> Self {
        Self {
            bids: VecDeque::new(),
            asks: VecDeque::new(),
            metadata,
            last_match_at: None,
            last_warning_at: None,
        }
    }

    fn push_bid(&mut self, bid: Bid) {
        let entry = QueuedBid::new(bid);
        insert_bid(&mut self.bids, entry);
    }

    fn push_ask(&mut self, ask: Ask) {
        let entry = QueuedAsk::new(ask);
        insert_ask(&mut self.asks, entry);
    }
}

struct OrderBook {
    lanes: DashMap<FeeLane, LaneState>,
}

impl Default for OrderBook {
    fn default() -> Self {
        Self {
            lanes: DashMap::new(),
        }
    }
}

impl OrderBook {
    fn replace(&self, seeds: Vec<LaneSeed>) -> Result<(), SeedError> {
        let mut staged = Vec::with_capacity(seeds.len());
        for seed in seeds {
            let LaneSeed {
                lane,
                bids,
                asks,
                metadata,
            } = seed;
            let max = metadata.max_queue_depth;
            if bids.len() > max || asks.len() > max {
                return Err(SeedError::CapacityExceeded {
                    lane,
                    attempted: bids.len().max(asks.len()),
                    max,
                });
            }
            let mut state = LaneState::new(metadata);
            for bid in bids {
                state.push_bid(bid);
            }
            for ask in asks {
                state.push_ask(ask);
            }
            staged.push((lane, state));
        }
        self.lanes.clear();
        for (lane, state) in staged {
            self.lanes.insert(lane, state);
        }
        Ok(())
    }

    fn lane_keys(&self) -> Vec<FeeLane> {
        let mut lanes: Vec<_> = self.lanes.keys();
        lanes.sort();
        lanes
    }

    fn match_batch(&self, batch: usize) -> Vec<MatchResult> {
        if batch == 0 {
            return Vec::new();
        }
        let lanes = self.lane_keys();
        let mut matched = Vec::new();
        loop {
            let mut progressed = false;
            for lane in &lanes {
                if matched.len() >= batch {
                    break;
                }
                if let Some(mut state) = self.lanes.get_mut(lane) {
                    let fairness_window = state.metadata.fairness_window;
                    let deadline = if fairness_window.is_zero() {
                        None
                    } else {
                        Some(Instant::now() + fairness_window)
                    };
                    let mut lane_progress = false;
                    while matched.len() < batch {
                        let Some(bid) = state.bids.front() else {
                            break;
                        };
                        let Some(ask) = state.asks.front() else {
                            break;
                        };
                        if bid.bid.price < ask.ask.price {
                            break;
                        }
                        if let Some(deadline) = deadline {
                            if lane_progress && Instant::now() > deadline {
                                break;
                            }
                        }
                        let bid = state.bids.pop_front().unwrap();
                        let ask = state.asks.pop_front().unwrap();
                        lane_progress = true;
                        state.last_match_at = Some(Instant::now());
                        matched.push(MatchResult {
                            lane: *lane,
                            bid: bid.bid,
                            ask: ask.ask,
                        });
                    }
                    if lane_progress {
                        state.last_warning_at = None;
                        progressed = true;
                    }
                }
            }
            if matched.len() >= batch || !progressed {
                break;
            }
        }
        matched
    }

    fn collect_starvation(&self, threshold: Duration) -> Vec<(LaneWarning, bool)> {
        let now = Instant::now();
        let wall_now = SystemTime::now();
        let mut warnings = Vec::new();
        for lane in self.lane_keys() {
            if let Some(mut state) = self.lanes.get_mut(&lane) {
                let front_info = state.bids.front().map(|front| {
                    (
                        front.bid.job_id.clone(),
                        now.saturating_duration_since(front.enqueued_at),
                    )
                });
                if let Some((oldest_job, waited)) = front_info {
                    if waited >= threshold {
                        let should_log = state
                            .last_warning_at
                            .map(|t| now.duration_since(t) >= threshold)
                            .unwrap_or(true);
                        if should_log {
                            state.last_warning_at = Some(now);
                        }
                        warnings.push((
                            LaneWarning {
                                lane,
                                oldest_job,
                                waited_for: waited,
                                updated_at: wall_now,
                            },
                            should_log,
                        ));
                    }
                } else {
                    state.last_warning_at = None;
                }
            }
        }
        warnings
    }

    fn lane_statuses(&self) -> Vec<LaneStatus> {
        let now = Instant::now();
        let mut statuses = Vec::new();
        for lane in self.lane_keys() {
            if let Some(state) = self.lanes.get(&lane) {
                let oldest_bid_wait = state
                    .bids
                    .front()
                    .map(|b| now.saturating_duration_since(b.enqueued_at));
                let oldest_ask_wait = state
                    .asks
                    .front()
                    .map(|a| now.saturating_duration_since(a.enqueued_at));
                statuses.push(LaneStatus {
                    lane,
                    bids: state.bids.len(),
                    asks: state.asks.len(),
                    oldest_bid_wait,
                    oldest_ask_wait,
                });
            }
        }
        statuses
    }

    fn snapshot(&self) -> Vec<LaneSnapshot> {
        let mut snaps = Vec::new();
        for lane in self.lane_keys() {
            if let Some(state) = self.lanes.get(&lane) {
                let bids = state.bids.iter().map(|b| b.bid.clone()).collect();
                let asks = state.asks.iter().map(|a| a.ask.clone()).collect();
                snaps.push(LaneSnapshot { lane, bids, asks });
            }
        }
        snaps
    }
}

struct MatchResult {
    lane: FeeLane,
    bid: Bid,
    ask: Ask,
}

static ORDER_BOOK: Lazy<OrderBook> = Lazy::new(OrderBook::default);
static STARVATION: Lazy<DashMap<FeeLane, LaneWarning>> = Lazy::new(DashMap::new);
static RECEIPT_STORE: Lazy<RwLock<Option<ReceiptStore>>> = Lazy::new(|| RwLock::new(None));

const MATCH_INTERVAL: Duration = Duration::from_millis(10);
const DEFAULT_BATCH_SIZE: usize = 32;
const DEFAULT_LANE_CAPACITY: usize = 1024;
const DEFAULT_FAIRNESS_WINDOW_MS: u64 = 5;
const DEFAULT_STARVATION_THRESHOLD_SECS: u64 = 30;

fn default_fairness_window() -> Duration {
    let ms = env::var("TB_COMPUTE_FAIRNESS_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_FAIRNESS_WINDOW_MS);
    Duration::from_millis(ms)
}

fn default_lane_capacity() -> usize {
    env::var("TB_COMPUTE_LANE_CAP")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_LANE_CAPACITY)
}

fn batch_size() -> usize {
    env::var("TB_COMPUTE_MATCH_BATCH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_BATCH_SIZE)
}

fn starvation_threshold() -> Duration {
    let secs = env::var("TB_COMPUTE_STARVATION_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_STARVATION_THRESHOLD_SECS);
    Duration::from_secs(secs)
}

pub fn seed_orders(lanes: Vec<LaneSeed>) -> Result<(), SeedError> {
    ORDER_BOOK.replace(lanes)?;
    STARVATION.clear();
    Ok(())
}

pub fn snapshot() -> Vec<LaneSnapshot> {
    ORDER_BOOK.snapshot()
}

pub fn lane_statuses() -> Vec<LaneStatus> {
    ORDER_BOOK.lane_statuses()
}

pub fn starvation_warnings() -> Vec<LaneWarning> {
    STARVATION.values()
}

pub fn recent_matches(lane: FeeLane, limit: usize) -> Vec<Receipt> {
    if let Ok(guard) = RECEIPT_STORE.read() {
        if let Some(store) = guard.as_ref() {
            return store.recent_by_lane(lane, limit).unwrap_or_default();
        }
    }
    Vec::new()
}

fn stable_match(batch: usize) -> Vec<MatchResult> {
    ORDER_BOOK.match_batch(batch)
}

fn refresh_starvation() {
    let threshold = starvation_threshold();
    let warnings = ORDER_BOOK.collect_starvation(threshold);
    let mut keep = std::collections::HashSet::new();
    for (warning, should_log) in warnings {
        keep.insert(warning.lane);
        STARVATION.insert(warning.lane, warning.clone());
        if should_log {
            #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
            diagnostics::tracing::warn!(lane = %warning.lane, job = %warning.oldest_job, waited = warning.waited_for.as_secs_f32(), "lane starvation");
        }
    }
    STARVATION.retain(|lane, _| keep.contains(lane));
}

/// Continuously attempt to match bids and asks, emitting receipts.
pub async fn match_loop(store: ReceiptStore, dry_run: bool, stop: CancellationToken) {
    let batch = batch_size();
    {
        let mut guard = RECEIPT_STORE.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(store.clone());
    }
    while !stop.is_cancelled() {
        let start = Instant::now();
        let matches = stable_match(batch);
        let mut touched_lanes = std::collections::HashSet::new();
        for matched in matches.iter() {
            touched_lanes.insert(matched.lane);
            let receipt = Receipt::new(
                matched.bid.job_id.clone(),
                matched.bid.buyer.clone(),
                matched.ask.provider.clone(),
                matched.ask.price,
                1,
                dry_run,
                matched.lane,
            );
            match store.try_insert(&receipt) {
                Ok(true) => {
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::MATCHES_TOTAL
                        .ensure_handle_for_label_values(&[
                            if dry_run { "true" } else { "false" },
                            matched.lane.as_str(),
                        ])
                        .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                        .inc();
                    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                    diagnostics::tracing::info!(
                        job = %receipt.job_id,
                        buyer = %receipt.buyer,
                        provider = %receipt.provider,
                        price = receipt.quote_price,
                        dry = receipt.dry_run,
                        lane = %matched.lane,
                        "match"
                    );
                }
                Ok(false) => {}
                Err(err) => {
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::RECEIPT_PERSIST_FAIL_TOTAL.inc();
                    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                    diagnostics::tracing::error!("receipt insert failed: {err}");
                    #[cfg(all(not(feature = "telemetry"), not(feature = "test-telemetry")))]
                    let _ = err;
                }
            }
        }
        #[cfg(feature = "telemetry")]
        {
            let elapsed = start.elapsed().as_secs_f64();
            for lane in &touched_lanes {
                crate::telemetry::MATCH_LOOP_LATENCY_SECONDS
                    .ensure_handle_for_label_values(&[lane.as_str()])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .observe(elapsed);
            }
        }
        #[cfg(not(feature = "telemetry"))]
        {
            let _ = start;
            let _ = &touched_lanes;
        }
        refresh_starvation();
        if matches.len() >= batch && batch > 0 {
            yield_now().await;
        } else {
            runtime::sleep(MATCH_INTERVAL).await;
        }
    }
    let mut guard = RECEIPT_STORE.write().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

fn insert_bid(queue: &mut VecDeque<QueuedBid>, bid: QueuedBid) {
    if queue.is_empty() {
        queue.push_back(bid);
        return;
    }
    let position = queue
        .iter()
        .position(|existing| {
            bid.bid.price > existing.bid.price
                || (bid.bid.price == existing.bid.price && bid.enqueued_at <= existing.enqueued_at)
        })
        .unwrap_or(queue.len());
    queue.insert(position, bid);
}

fn insert_ask(queue: &mut VecDeque<QueuedAsk>, ask: QueuedAsk) {
    if queue.is_empty() {
        queue.push_back(ask);
        return;
    }
    let position = queue
        .iter()
        .position(|existing| {
            ask.ask.price < existing.ask.price
                || (ask.ask.price == existing.ask.price && ask.enqueued_at <= existing.enqueued_at)
        })
        .unwrap_or(queue.len());
    queue.insert(position, ask);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn seed_orders_populates_order_books() {
        let bid = Bid {
            job_id: "job-1".into(),
            buyer: "buyer".into(),
            price: 10,
            lane: FeeLane::Consumer,
        };
        let ask = Ask {
            job_id: "job-1".into(),
            provider: "provider".into(),
            price: 10,
            lane: FeeLane::Consumer,
        };

        seed_orders(vec![LaneSeed {
            lane: FeeLane::Consumer,
            bids: vec![bid.clone()],
            asks: vec![ask.clone()],
            metadata: LaneMetadata::default(),
        }])
        .unwrap();

        let snapshot = snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].bids.len(), 1);
        assert_eq!(snapshot[0].asks.len(), 1);
        assert_eq!(snapshot[0].bids[0].job_id, bid.job_id);
        assert_eq!(snapshot[0].asks[0].provider, ask.provider);
    }

    #[test]
    fn seed_orders_error_preserves_previous_state() {
        seed_orders(vec![LaneSeed {
            lane: FeeLane::Consumer,
            bids: vec![Bid {
                job_id: "job-1".into(),
                buyer: "buyer".into(),
                price: 10,
                lane: FeeLane::Consumer,
            }],
            asks: Vec::new(),
            metadata: LaneMetadata::default(),
        }])
        .unwrap();

        let err = seed_orders(vec![LaneSeed {
            lane: FeeLane::Industrial,
            bids: vec![
                Bid {
                    job_id: "job-a".into(),
                    buyer: "buyer".into(),
                    price: 5,
                    lane: FeeLane::Industrial,
                },
                Bid {
                    job_id: "job-b".into(),
                    buyer: "buyer".into(),
                    price: 4,
                    lane: FeeLane::Industrial,
                },
            ],
            asks: Vec::new(),
            metadata: LaneMetadata {
                fairness_window: Duration::from_millis(1),
                max_queue_depth: 1,
            },
        }])
        .unwrap_err();

        assert!(matches!(err, SeedError::CapacityExceeded { .. }));
        let snapshot = snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].lane, FeeLane::Consumer);
        assert_eq!(snapshot[0].bids.len(), 1);

        seed_orders(Vec::new()).unwrap();
    }
}

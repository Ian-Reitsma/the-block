use crate::transaction::FeeLane;
use concurrency::{Lazy, OnceCell};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread;
use std::time::Duration;
#[cfg(feature = "telemetry")]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::util::atomic_file::write_atomic;
use crate::util::clock::{Clock, MonotonicClock};
use crate::util::versioned_blob::{decode_blob, encode_blob, DecodeErr, MAGIC_PRICE_BOARD};
use foundation_serialization::binary;

#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
use diagnostics::tracing::{info, warn};
#[cfg(feature = "telemetry")]
use runtime::telemetry::{IntCounterVec, IntGauge, IntGaugeVec, Opts};

#[cfg(feature = "telemetry")]
use crate::telemetry::{INDUSTRIAL_BACKLOG, INDUSTRIAL_PRICE_PER_UNIT, INDUSTRIAL_UTILIZATION};

const MAGIC: [u8; 4] = MAGIC_PRICE_BOARD;
const VERSION: u16 = 3;

/// Sliding window of recent prices with quantile bands per lane.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(crate = "foundation_serialization::serde")]
struct PriceEntry {
    price: u64,
    weighted: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PriceBoard {
    pub window: usize,
    consumer: VecDeque<PriceEntry>,
    industrial: VecDeque<PriceEntry>,
}

impl PriceBoard {
    pub fn new(window: usize) -> Self {
        Self {
            window,
            consumer: VecDeque::with_capacity(window),
            industrial: VecDeque::with_capacity(window),
        }
    }

    pub fn record(&mut self, lane: FeeLane, price: u64, multiplier: f64) {
        let prices = match lane {
            FeeLane::Consumer => &mut self.consumer,
            FeeLane::Industrial => &mut self.industrial,
        };
        if prices.len() == self.window {
            prices.pop_front();
        }
        let weighted = (price as f64 * multiplier).round() as u64;
        prices.push_back(PriceEntry { price, weighted });
        #[cfg(feature = "telemetry")]
        if let FeeLane::Industrial = lane {
            INDUSTRIAL_PRICE_PER_UNIT.set(weighted as i64);
        }
        if multiplier != 1.0 {
            #[cfg(feature = "telemetry")]
            crate::telemetry::PRICE_WEIGHT_APPLIED_TOTAL.inc();
        }
        self.update_metrics(lane);
    }

    fn update_metrics(&self, lane: FeeLane) {
        #[cfg(not(feature = "telemetry"))]
        let _ = lane;
        #[cfg(feature = "telemetry")]
        if let Some((p25, med, p75)) = self.bands(lane) {
            let l = match lane {
                FeeLane::Consumer => "consumer",
                FeeLane::Industrial => "industrial",
            };
            PRICE_BAND_P25
                .ensure_handle_for_label_values(&[l])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .set(p25 as i64);
            PRICE_BAND_MEDIAN
                .ensure_handle_for_label_values(&[l])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .set(med as i64);
            PRICE_BAND_P75
                .ensure_handle_for_label_values(&[l])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .set(p75 as i64);
        }
    }

    /// Return p25, median and p75 bands for a lane.
    pub fn bands(&self, lane: FeeLane) -> Option<(u64, u64, u64)> {
        let prices = match lane {
            FeeLane::Consumer => &self.consumer,
            FeeLane::Industrial => &self.industrial,
        };
        if prices.is_empty() {
            return None;
        }
        let mut v: Vec<_> = prices.iter().map(|e| e.weighted).collect();
        v.sort_unstable();
        let median = v[v.len() / 2];
        let p25 = v[(v.len() as f64 * 0.25).floor() as usize];
        let p75 = v[(v.len() as f64 * 0.75).floor() as usize];
        Some((p25, median, p75))
    }

    pub fn backlog_adjusted_bid(&self, lane: FeeLane, backlog: usize) -> Option<u64> {
        let (_, median, _) = self.bands(lane)?;
        let factor = 1.0 + backlog as f64 / self.window as f64;
        Some((median as f64 * factor).ceil() as u64)
    }

    pub fn raw_bands(&self, lane: FeeLane) -> Option<(u64, u64, u64)> {
        let prices = match lane {
            FeeLane::Consumer => &self.consumer,
            FeeLane::Industrial => &self.industrial,
        };
        if prices.is_empty() {
            return None;
        }
        let mut v: Vec<_> = prices.iter().map(|e| e.price).collect();
        v.sort_unstable();
        let median = v[v.len() / 2];
        let p25 = v[(v.len() as f64 * 0.25).floor() as usize];
        let p75 = v[(v.len() as f64 * 0.75).floor() as usize];
        Some((p25, median, p75))
    }

    fn clear(&mut self) {
        self.consumer.clear();
        self.industrial.clear();
    }
}

static BOARD: Lazy<RwLock<PriceBoard>> = Lazy::new(|| RwLock::new(PriceBoard::new(100)));
static BOARD_PATH: OnceCell<Mutex<Option<PathBuf>>> = OnceCell::new();
static SAVE_STOP: OnceCell<Mutex<Option<Arc<AtomicBool>>>> = OnceCell::new();
static SAVE_HANDLE: OnceCell<Mutex<Option<thread::JoinHandle<()>>>> = OnceCell::new();

fn board_path() -> Option<PathBuf> {
    BOARD_PATH
        .get()
        .and_then(|m| m.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

fn stop_saver() {
    if let Some(cell) = SAVE_STOP.get() {
        if let Some(stop) = cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
            stop.store(true, Ordering::SeqCst);
        }
    }
    if let Some(cell) = SAVE_HANDLE.get() {
        if let Some(handle) = cell.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = handle.join();
        }
    }
}

fn save_to_path(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let board = BOARD.read().unwrap_or_else(|e| e.into_inner());
    let payload = binary::encode(&*board).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let blob = encode_blob(MAGIC, VERSION, &payload)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    write_atomic(path, &blob)
}

fn save_with_metrics(path: &Path) {
    match save_to_path(path) {
        Ok(()) => {
            #[cfg(feature = "telemetry")]
            {
                PRICE_BOARD_SAVE_TOTAL
                    .ensure_handle_for_label_values(&["ok"])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .inc();
                if let Ok(epoch) = SystemTime::now().duration_since(UNIX_EPOCH) {
                    PRICE_BOARD_LAST_SAVE_EPOCH.set(epoch.as_secs() as i64);
                }
            }
        }
        Err(err) => {
            #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
            warn!("failed to write price board {}: {err}", path.display());
            #[cfg(feature = "telemetry")]
            PRICE_BOARD_SAVE_TOTAL
                .ensure_handle_for_label_values(&["io_err"])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .inc();
            #[cfg(not(any(feature = "telemetry", feature = "test-telemetry")))]
            let _ = err;
        }
    }
}

static BACKLOG: AtomicU64 = AtomicU64::new(0);
static UTILIZATION: AtomicU64 = AtomicU64::new(0);

/// Record current backlog and utilisation metrics.
pub fn report_backlog(backlog: u64, capacity: u64) {
    BACKLOG.store(backlog, Ordering::Relaxed);
    let util = if capacity == 0 {
        0
    } else {
        ((backlog as f64 / capacity as f64) * 100.0).round() as u64
    };
    UTILIZATION.store(util, Ordering::Relaxed);
    #[cfg(feature = "telemetry")]
    {
        INDUSTRIAL_BACKLOG.set(backlog as i64);
        INDUSTRIAL_UTILIZATION.set(util as i64);
    }
}

/// Snapshot the backlog and utilisation metrics.
pub fn backlog_utilization() -> (u64, u64) {
    (
        BACKLOG.load(Ordering::Relaxed),
        UTILIZATION.load(Ordering::Relaxed),
    )
}

fn spawn_saver<C: Clock>(path: PathBuf, interval: Duration, clock: C) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let handle = thread::spawn(move || {
        let mut last = clock.now();
        while !stop_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
            let now = clock.now();
            if now.duration_since(last) >= interval {
                last = now;
                save_with_metrics(&path);
            }
        }
    });
    SAVE_STOP
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(stop);
    SAVE_HANDLE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(handle);
}

pub fn init(path: String, window: usize, save_interval_secs: u64) {
    init_with_clock(path, window, save_interval_secs, MonotonicClock::default());
}

pub fn init_with_clock<C: Clock>(path: String, window: usize, save_interval_secs: u64, clock: C) {
    stop_saver();
    let cell = BOARD_PATH.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(PathBuf::from(&path));
    let path_buf = PathBuf::from(path);
    let result = match std::fs::read(&path_buf) {
        Ok(bytes) => match decode_blob(&bytes, MAGIC) {
            Ok((ver, payload)) => {
                if ver == VERSION {
                    match binary::decode::<PriceBoard>(payload) {
                        Ok(saved) => {
                            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
                            *guard = saved;
                            guard.update_metrics(FeeLane::Consumer);
                            guard.update_metrics(FeeLane::Industrial);
                            "ok"
                        }
                        Err(_) => {
                            #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                            warn!(
                                "failed to deserialize price board {}; starting empty",
                                path_buf.display()
                            );
                            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
                            *guard = PriceBoard::new(window);
                            "corrupt"
                        }
                    }
                } else {
                    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                    warn!("unsupported price board version {ver}; attempting migrate");
                    match migrate(ver, payload) {
                        Ok(state) => {
                            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
                            *guard = state;
                            guard.update_metrics(FeeLane::Consumer);
                            guard.update_metrics(FeeLane::Industrial);
                        }
                        Err(_) => {
                            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
                            *guard = PriceBoard::new(window);
                        }
                    }
                    "version_migrate"
                }
            }
            Err(
                DecodeErr::BadMagic
                | DecodeErr::BadCrc
                | DecodeErr::UnsupportedVersion { .. }
                | DecodeErr::Unimplemented(_),
            ) => {
                #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                warn!(
                    "corrupted price board {}; starting empty",
                    path_buf.display()
                );
                let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
                *guard = PriceBoard::new(window);
                "corrupt"
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
            info!("no price board at {}; starting empty", path_buf.display());
            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
            *guard = PriceBoard::new(window);
            "missing"
        }
        Err(_) => {
            #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
            warn!(
                "failed to read price board {}; starting empty",
                path_buf.display()
            );
            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
            *guard = PriceBoard::new(window);
            "corrupt"
        }
    };
    #[cfg(feature = "telemetry")]
    PRICE_BOARD_LOAD_TOTAL
        .ensure_handle_for_label_values(&[result])
        .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
        .inc();
    #[cfg(not(feature = "telemetry"))]
    let _ = result;
    spawn_saver(path_buf, Duration::from_secs(save_interval_secs), clock);
}

pub fn persist() {
    stop_saver();
    if let Some(path) = board_path() {
        save_with_metrics(&path);
    }
}

/// Record a new price into the global board for a lane.
pub fn record_price(lane: FeeLane, price: u64, multiplier: f64) {
    if let Ok(mut b) = BOARD.write() {
        b.record(lane, price, multiplier);
    }
}

/// Fetch current bands.
pub fn bands(lane: FeeLane) -> Option<(u64, u64, u64)> {
    BOARD.read().ok().and_then(|b| b.bands(lane))
}

pub fn raw_bands(lane: FeeLane) -> Option<(u64, u64, u64)> {
    BOARD.read().ok().and_then(|b| b.raw_bands(lane))
}

/// Return the most recent spot price (weighted median if available).
pub fn spot_price_per_unit(lane: FeeLane) -> Option<u64> {
    BOARD.read().ok().and_then(|b| {
        b.bands(lane)
            .map(|(_, median, _)| median)
            .or_else(|| b.raw_bands(lane).map(|(_, median, _)| median))
    })
}

/// Compute backlog adjusted bid using current bands for a lane.
pub fn backlog_adjusted_bid(lane: FeeLane, backlog: usize) -> Option<u64> {
    BOARD
        .read()
        .ok()
        .and_then(|b| b.backlog_adjusted_bid(lane, backlog))
}

pub fn reset() {
    if let Ok(mut b) = BOARD.write() {
        b.clear();
    }
}

#[doc(hidden)]
/// Clear the persisted path so tests can reinitialize the board with a fresh file.
pub fn reset_path_for_test() {
    stop_saver();
    if let Some(lock) = BOARD_PATH.get() {
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[derive(Debug)]
pub enum MigrateErr {
    Unsupported(u16),
}

impl fmt::Display for MigrateErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrateErr::Unsupported(ver) => write!(f, "unsupported version {ver}"),
        }
    }
}

impl std::error::Error for MigrateErr {}

fn migrate(from_ver: u16, bytes: &[u8]) -> Result<PriceBoard, MigrateErr> {
    if from_ver == 1 {
        #[derive(Deserialize)]
        struct V1 {
            window: usize,
            consumer: VecDeque<u64>,
            industrial: VecDeque<u64>,
        }
        let v1: V1 = binary::decode(bytes).map_err(|_| MigrateErr::Unsupported(from_ver))?;
        Ok(PriceBoard {
            window: v1.window,
            consumer: v1
                .consumer
                .into_iter()
                .map(|p| PriceEntry {
                    price: p,
                    weighted: p,
                })
                .collect(),
            industrial: v1
                .industrial
                .into_iter()
                .map(|p| PriceEntry {
                    price: p,
                    weighted: p,
                })
                .collect(),
        })
    } else if from_ver == 2 {
        #[derive(Deserialize)]
        struct V2Entry {
            price: u64,
            multiplier: f64,
        }
        #[derive(Deserialize)]
        struct V2 {
            window: usize,
            consumer: VecDeque<V2Entry>,
            industrial: VecDeque<V2Entry>,
        }
        let v2: V2 = binary::decode(bytes).map_err(|_| MigrateErr::Unsupported(from_ver))?;
        Ok(PriceBoard {
            window: v2.window,
            consumer: v2
                .consumer
                .into_iter()
                .map(|e| PriceEntry {
                    price: e.price,
                    weighted: (e.price as f64 * e.multiplier).round() as u64,
                })
                .collect(),
            industrial: v2
                .industrial
                .into_iter()
                .map(|e| PriceEntry {
                    price: e.price,
                    weighted: (e.price as f64 * e.multiplier).round() as u64,
                })
                .collect(),
        })
    } else {
        Err(MigrateErr::Unsupported(from_ver))
    }
}

#[cfg(feature = "telemetry")]
static PRICE_BAND_P25: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("price_band_p25", "25th percentile compute price"),
        &["lane"],
    )
    .unwrap_or_else(|e| panic!("gauge p25: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("register p25 gauge: {e}"));
    g
});

#[cfg(feature = "telemetry")]
static PRICE_BAND_MEDIAN: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("price_band_median", "Median compute price"),
        &["lane"],
    )
    .unwrap_or_else(|e| panic!("gauge median: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("register median gauge: {e}"));
    g
});

#[cfg(feature = "telemetry")]
static PRICE_BAND_P75: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("price_band_p75", "75th percentile compute price"),
        &["lane"],
    )
    .unwrap_or_else(|e| panic!("gauge p75: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("register p75 gauge: {e}"));
    g
});

#[cfg(feature = "telemetry")]
static PRICE_BOARD_LOAD_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("price_board_load_total", "price board load attempts"),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("load counter: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("register load counter: {e}"));
    c
});

#[cfg(feature = "telemetry")]
static PRICE_BOARD_SAVE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("price_board_save_total", "price board save attempts"),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("save counter: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("register save counter: {e}"));
    c
});

#[cfg(feature = "telemetry")]
static PRICE_BOARD_LAST_SAVE_EPOCH: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new(
        "price_board_last_save_epoch",
        "unix epoch of last successful price board save",
    )
    .unwrap_or_else(|e| panic!("last-save gauge: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("register last-save gauge: {e}"));
    g
});

use once_cell::sync::{Lazy, OnceCell};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread;
use std::time::Duration;
#[cfg(feature = "telemetry")]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::util::atomic_file::write_atomic;
use crate::util::clock::{Clock, MonotonicClock};
use crate::util::versioned_blob::{decode_blob, encode_blob, DecodeErr, MAGIC_PRICE_BOARD};

#[cfg(feature = "telemetry")]
use prometheus::{IntCounterVec, IntGauge, Opts};
#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
use tracing::{info, warn};

const MAGIC: [u8; 4] = MAGIC_PRICE_BOARD;
const VERSION: u16 = 1;

/// Sliding window of recent prices with quantile bands.
#[derive(Serialize, Deserialize)]
pub struct PriceBoard {
    pub window: usize,
    pub prices: VecDeque<u64>,
}

impl PriceBoard {
    pub fn new(window: usize) -> Self {
        Self {
            window,
            prices: VecDeque::with_capacity(window),
        }
    }

    pub fn record(&mut self, price: u64) {
        if self.prices.len() == self.window {
            self.prices.pop_front();
        }
        self.prices.push_back(price);
        self.update_metrics();
    }

    fn update_metrics(&self) {
        #[cfg(feature = "telemetry")]
        if let Some((p25, med, p75)) = self.bands() {
            PRICE_P25.set(p25 as i64);
            PRICE_MEDIAN.set(med as i64);
            PRICE_P75.set(p75 as i64);
        }
    }

    /// Return p25, median and p75 bands.
    pub fn bands(&self) -> Option<(u64, u64, u64)> {
        if self.prices.is_empty() {
            return None;
        }
        let mut v: Vec<_> = self.prices.iter().copied().collect();
        v.sort_unstable();
        let median = v[v.len() / 2];
        let p25 = v[(v.len() as f64 * 0.25).floor() as usize];
        let p75 = v[(v.len() as f64 * 0.75).floor() as usize];
        Some((p25, median, p75))
    }

    pub fn backlog_adjusted_bid(&self, backlog: usize) -> Option<u64> {
        let (_, median, _) = self.bands()?;
        let factor = 1.0 + backlog as f64 / self.window as f64;
        Some((median as f64 * factor).ceil() as u64)
    }

    fn clear(&mut self) {
        self.prices.clear();
        self.update_metrics();
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
    let payload =
        bincode::serialize(&*board).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let blob = encode_blob(MAGIC, VERSION, &payload);
    write_atomic(path, &blob)
}

fn save_with_metrics(path: &Path) {
    match save_to_path(path) {
        Ok(()) => {
            #[cfg(feature = "telemetry")]
            {
                PRICE_BOARD_SAVE_TOTAL.with_label_values(&["ok"]).inc();
                if let Ok(epoch) = SystemTime::now().duration_since(UNIX_EPOCH) {
                    PRICE_BOARD_LAST_SAVE_EPOCH.set(epoch.as_secs() as i64);
                }
            }
        }
        Err(err) => {
            #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
            warn!("failed to write price board {}: {err}", path.display());
            #[cfg(feature = "telemetry")]
            PRICE_BOARD_SAVE_TOTAL.with_label_values(&["io_err"]).inc();
            #[cfg(not(any(feature = "telemetry", feature = "test-telemetry")))]
            let _ = err;
        }
    }
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
                    match bincode::deserialize::<PriceBoard>(payload) {
                        Ok(saved) => {
                            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
                            *guard = saved;
                            guard.update_metrics();
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
                            guard.update_metrics();
                        }
                        Err(_) => {
                            let mut guard = BOARD.write().unwrap_or_else(|e| e.into_inner());
                            *guard = PriceBoard::new(window);
                        }
                    }
                    "version_migrate"
                }
            }
            Err(DecodeErr::BadMagic | DecodeErr::BadCrc | DecodeErr::UnsupportedVersion { .. }) => {
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
    PRICE_BOARD_LOAD_TOTAL.with_label_values(&[result]).inc();
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

/// Record a new price into the global board.
pub fn record_price(price: u64) {
    if let Ok(mut b) = BOARD.write() {
        b.record(price);
    }
}

/// Fetch current bands.
pub fn bands() -> Option<(u64, u64, u64)> {
    BOARD.read().ok().and_then(|b| b.bands())
}

/// Compute backlog adjusted bid using current bands.
pub fn backlog_adjusted_bid(backlog: usize) -> Option<u64> {
    BOARD
        .read()
        .ok()
        .and_then(|b| b.backlog_adjusted_bid(backlog))
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

#[derive(Debug, thiserror::Error)]
pub enum MigrateErr {
    #[error("unsupported version {0}")]
    Unsupported(u16),
}

fn migrate(from_ver: u16, _bytes: &[u8]) -> Result<PriceBoard, MigrateErr> {
    Err(MigrateErr::Unsupported(from_ver))
}

#[cfg(feature = "telemetry")]
static PRICE_P25: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("price_band_p25", "25th percentile compute price")
        .unwrap_or_else(|e| panic!("gauge p25: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("register p25 gauge: {e}"));
    g
});

#[cfg(feature = "telemetry")]
static PRICE_MEDIAN: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("price_band_median", "Median compute price")
        .unwrap_or_else(|e| panic!("gauge median: {e}"));
    crate::telemetry::REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("register median gauge: {e}"));
    g
});

#[cfg(feature = "telemetry")]
static PRICE_P75: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("price_band_p75", "75th percentile compute price")
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

use once_cell::sync::{Lazy, OnceCell};
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(feature = "telemetry")]
use prometheus::IntGauge;
#[cfg(feature = "telemetry")]
use tracing::{info, warn};

/// Sliding window of recent prices with quantile bands.
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

static BOARD: Lazy<Mutex<PriceBoard>> = Lazy::new(|| Mutex::new(PriceBoard::new(100)));
static BOARD_PATH: OnceCell<Mutex<Option<PathBuf>>> = OnceCell::new();

fn set_path<P: Into<PathBuf>>(p: P) {
    let cell = BOARD_PATH.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap();
    if guard.is_none() {
        *guard = Some(p.into());
    }
}

pub fn init(path: String, window: usize) {
    set_path(path.clone());
    if let Ok(mut b) = BOARD.lock() {
        b.window = window;
        b.prices = VecDeque::with_capacity(window);
        let path_buf = PathBuf::from(&path);
        match fs::read(&path_buf) {
            Ok(bytes) => match bincode::deserialize::<VecDeque<u64>>(&bytes) {
                Ok(saved) => {
                    #[cfg(feature = "telemetry")]
                    info!("loaded price board from {}", path);
                    b.prices = saved;
                }
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    warn!("failed to parse price board {}: {e}; starting empty", path);
                    #[cfg(not(feature = "telemetry"))]
                    let _ = e;
                }
            },
            Err(_) => {
                #[cfg(feature = "telemetry")]
                info!("no price board at {}; starting empty", path);
            }
        }
        b.update_metrics();
    }
}

pub fn persist() {
    if let Some(lock) = BOARD_PATH.get() {
        if let Some(path) = lock.lock().unwrap().clone() {
            if let Ok(b) = BOARD.lock() {
                if let Ok(bytes) = bincode::serialize(&b.prices) {
                    if let Err(e) = fs::write(&path, bytes) {
                        #[cfg(feature = "telemetry")]
                        warn!("failed to write price board {}: {e}", path.display());
                        #[cfg(not(feature = "telemetry"))]
                        let _ = e;
                    } else {
                        #[cfg(feature = "telemetry")]
                        info!("saved price board to {}", path.display());
                    }
                }
            }
        }
    }
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

/// Record a new price into the global board.
pub fn record_price(price: u64) {
    if let Ok(mut b) = BOARD.lock() {
        b.record(price);
    }
}

/// Fetch current bands.
pub fn bands() -> Option<(u64, u64, u64)> {
    BOARD.lock().ok().and_then(|b| b.bands())
}

/// Compute backlog adjusted bid using current bands.
pub fn backlog_adjusted_bid(backlog: usize) -> Option<u64> {
    BOARD
        .lock()
        .ok()
        .and_then(|b| b.backlog_adjusted_bid(backlog))
}

pub fn reset() {
    if let Ok(mut b) = BOARD.lock() {
        b.clear();
    }
}

#[doc(hidden)]
/// Clear the persisted path so tests can reinitialize the board with a fresh file.
pub fn reset_path_for_test() {
    if let Some(lock) = BOARD_PATH.get() {
        *lock.lock().unwrap() = None;
    }
}

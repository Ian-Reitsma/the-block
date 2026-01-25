use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketMode {
    Rehearsal = 0,
    Trade = 1,
}

impl MarketMode {
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled {
            MarketMode::Trade
        } else {
            MarketMode::Rehearsal
        }
    }
}

static STORAGE_MODE: AtomicU8 = AtomicU8::new(MarketMode::Rehearsal as u8);
static COMPUTE_MODE: AtomicU8 = AtomicU8::new(MarketMode::Rehearsal as u8);
static ENERGY_MODE: AtomicU8 = AtomicU8::new(MarketMode::Rehearsal as u8);

fn load_mode(cell: &AtomicU8) -> MarketMode {
    match cell.load(Ordering::Relaxed) {
        1 => MarketMode::Trade,
        _ => MarketMode::Rehearsal,
    }
}

pub fn storage_mode() -> MarketMode {
    load_mode(&STORAGE_MODE)
}

pub fn compute_mode() -> MarketMode {
    load_mode(&COMPUTE_MODE)
}

pub fn energy_mode() -> MarketMode {
    load_mode(&ENERGY_MODE)
}

pub fn set_storage_mode(mode: MarketMode) {
    STORAGE_MODE.store(mode as u8, Ordering::Relaxed);
}

pub fn set_compute_mode(mode: MarketMode) {
    COMPUTE_MODE.store(mode as u8, Ordering::Relaxed);
}

pub fn set_energy_mode(mode: MarketMode) {
    ENERGY_MODE.store(mode as u8, Ordering::Relaxed);
}

/// Align gate state with the persisted governance params.
pub fn sync_from_params(params: &crate::governance::Params) {
    set_storage_mode(MarketMode::from_enabled(params.launch_storage_autopilot != 0));
    set_compute_mode(MarketMode::from_enabled(params.launch_compute_autopilot != 0));
    set_energy_mode(MarketMode::from_enabled(params.launch_energy_autopilot != 0));
}

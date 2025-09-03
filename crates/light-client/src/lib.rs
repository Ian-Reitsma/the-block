#![forbid(unsafe_code)]

/// Options controlling background synchronization.
#[derive(Clone, Copy)]
pub struct SyncOptions {
    pub wifi_only: bool,
    pub require_charging: bool,
    pub min_battery: f32,
}

/// Attempt a background sync respecting the provided `SyncOptions`.
///
/// The actual network and power checks are currently stubs and should be
/// replaced with platform-specific integrations in the mobile SDKs.
pub fn sync_background(opts: SyncOptions) {
    if opts.wifi_only && !on_wifi() {
        return;
    }
    if opts.require_charging && !is_charging() {
        return;
    }
    if battery_level() < opts.min_battery {
        return;
    }
    // perform sync here
}

fn on_wifi() -> bool {
    true
}

fn is_charging() -> bool {
    true
}

fn battery_level() -> f32 {
    1.0
}

/// Simplified block header used by the demo light client.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Header {
    pub height: u64,
}

/// Naive light client tracking headers.
pub struct LightClient {
    pub chain: Vec<Header>,
}

impl LightClient {
    pub fn new(genesis: Header) -> Self {
        Self {
            chain: vec![genesis],
        }
    }

    pub fn verify_and_append(&mut self, h: Header) -> Result<(), ()> {
        self.chain.push(h);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_thresholds() {
        let opts = SyncOptions {
            wifi_only: true,
            require_charging: true,
            min_battery: 0.5,
        };
        sync_background(opts);
    }
}

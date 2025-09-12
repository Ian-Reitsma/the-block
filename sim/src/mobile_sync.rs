use light_client::{sync_background, Header, LightClient, SyncOptions};
use std::time::Instant;

/// Measure the time required to sync a batch of headers on a simulated mobile client.
pub fn measure_sync_latency(headers: Vec<Header>) -> std::time::Duration {
    let mut iter = headers.into_iter();
    let genesis = match iter.next() {
        Some(h) => h,
        None => return std::time::Duration::default(),
    };
    let mut client = LightClient::new(genesis);
    let remaining: Vec<Header> = iter.collect();
    let fetch = move |start: u64| {
        remaining
            .clone()
            .into_iter()
            .filter(|h| h.height >= start)
            .collect()
    };
    let opts = SyncOptions {
        wifi_only: false,
        require_charging: false,
        min_battery: 0.0,
    };
    let start = Instant::now();
    sync_background(&mut client, opts, fetch);
    start.elapsed()
}

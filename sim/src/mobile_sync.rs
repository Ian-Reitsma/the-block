use light_client::{sync_background, Header, LightClient, SyncOptions};
use std::time::{Duration, Instant};

/// Measure the time required to sync a batch of headers on a simulated mobile client.
/// Each fetched header incurs an artificial `delay` to model network latency.
pub fn measure_sync_latency(headers: Vec<Header>, delay: Duration) -> Duration {
    let mut iter = headers.into_iter();
    let genesis = match iter.next() {
        Some(h) => h,
        None => return std::time::Duration::default(),
    };
    let mut client = LightClient::new(genesis);
    let remaining: Vec<Header> = iter.collect();
    let fetch = move |start: u64, _batch: usize| {
        let remaining = remaining.clone();
        async move {
            let mut out = Vec::new();
            for h in remaining.into_iter().filter(|h| h.height >= start) {
                std::thread::sleep(delay);
                out.push(h);
            }
            out
        }
    };
    let opts = SyncOptions {
        wifi_only: false,
        require_charging: false,
        min_battery: 0.0,
        ..SyncOptions::default()
    };
    let start = Instant::now();
    runtime::block_on(async { sync_background(&mut client, opts, fetch).await.unwrap() });
    start.elapsed()
}

use crypto_suite::hashing::blake3;
use foundation_profiler::ProfilerGuard;
use std::fs::File;
use std::time::Instant;

fn main() {
    let guard = ProfilerGuard::new(100).ok();
    let start = Instant::now();
    let mut digest = [0u8; 32];
    for _ in 0..100_000 {
        digest.copy_from_slice(blake3::hash(b"the-block").as_bytes());
    }
    let _elapsed = start.elapsed();
    if let Some(g) = guard {
        if let Ok(report) = g.report().build() {
            if let Ok(file) = File::create("auto_profile.svg") {
                let _ = report.flamegraph(file);
            }
        }
    }
}

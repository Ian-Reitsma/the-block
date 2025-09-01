use super::seed::record_seed;
use rand::thread_rng;
use rand::{rngs::StdRng, Rng, SeedableRng};

/// Randomize the RPC client timeout for tests to surface edge conditions.
#[allow(dead_code)]
pub fn randomize_client_timeout() {
    let seed = std::env::var("TB_RPC_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| thread_rng().gen());
    record_seed("rpc_timeout", seed);
    let mut rng = StdRng::seed_from_u64(seed);
    let secs: u64 = rng.gen_range(1..=5);
    std::env::set_var("TB_RPC_CLIENT_TIMEOUT_SECS", secs.to_string());
}

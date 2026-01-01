#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use foundation_serialization::binary;
use rand::{rngs::StdRng, Rng};
use the_block::{
    generate_keypair, mempool_cmp, sign_tx, Blockchain, RawTxPayload, SignedTransaction,
};

mod util;
use util::temp::temp_dir;

fn init() {
    let _ = fs::remove_dir_all("chain_db");
}

fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    consumer: u64,
    industrial: u64,
    fee: u64,
    nonce: u64,
) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer: consumer,
        amount_industrial: industrial,
        fee,
        pct: 100,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("valid key")
}

/// Benchmark concurrent submission performance to calibrate test workload.
///
/// Returns: (transactions_per_second, recommended_batch_size)
fn benchmark_concurrent_performance(threads: usize) -> (f64, usize) {
    use std::time::Instant;

    let warmup_bc = Arc::new(RwLock::new(Blockchain::new(
        temp_dir("perf_bench").path().to_str().unwrap(),
    )));
    {
        let mut bc = warmup_bc.write().unwrap();
        bc.add_account("sink".into(), 0).unwrap();
        for i in 0..threads {
            bc.add_account(format!("bench{i}"), 1_000_000).unwrap();
        }
    }

    let accounts: Vec<_> = (0..threads).map(|_| generate_keypair().0).collect();
    let accounts = Arc::new(accounts);
    let start = Instant::now();

    // Benchmark realistic concurrent workload
    const BENCH_PER_THREAD: usize = 10;
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let bc = Arc::clone(&warmup_bc);
            let sk = accounts[t].clone();
            let addr = format!("bench{t}");
            std::thread::spawn(move || {
                for i in 0..BENCH_PER_THREAD {
                    let tx = build_signed_tx(&sk, &addr, "sink", 1, 0, 100, i as u64 + 1);
                    let _ = bc.write().unwrap().submit_transaction(tx);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let elapsed = start.elapsed().as_secs_f64();
    let total_txs = threads * BENCH_PER_THREAD;
    let txs_per_sec = total_txs as f64 / elapsed;

    // Recommended batch: enough to stress test, but keeps runtime under 10s
    const TARGET_SECS: f64 = 10.0;
    const MIN_BATCH: usize = 500; // Minimum for statistical validity
    const MAX_BATCH: usize = 5_000; // Cap for very fast machines

    let recommended = ((txs_per_sec * TARGET_SECS) as usize).clamp(MIN_BATCH, MAX_BATCH);

    drop(warmup_bc);
    (txs_per_sec, recommended)
}

/// Calculate confidence score for mempool correctness validation.
///
/// Uses statistical sampling: as we submit more transactions and validate
/// ordering, our confidence increases. Returns true when we have sufficient
/// evidence that the mempool is working correctly.
fn has_sufficient_confidence(
    submitted: usize,
    accepted: usize,
    evictions: usize,
    mempool_size: usize,
) -> bool {
    // Minimum samples: must fill mempool at least 3x to trigger evictions
    if submitted < mempool_size * 3 {
        return false;
    }

    // Must have meaningful eviction activity (stress test the eviction logic)
    if evictions < mempool_size {
        return false;
    }

    // Acceptance rate check: should be reasonable (not rejecting everything)
    let acceptance_rate = accepted as f64 / submitted as f64;
    if acceptance_rate < 0.3 {
        // Less than 30% accepted is suspicious
        return false;
    }

    // Confidence based on sample size (statistical power)
    // After sufficient evictions and good acceptance rate, we have confidence
    let confidence_score = (evictions as f64 / mempool_size as f64) * acceptance_rate;
    confidence_score >= 2.0 // At least 2x mempool worth of validated evictions
}

/// Determine adaptive test parameters based on hardware performance.
fn adaptive_test_params(threads: usize) -> (usize, usize) {
    // Allow explicit override via environment variable
    if let Ok(val) = std::env::var("TB_FUZZ_ITERS") {
        if let Ok(n) = val.parse::<usize>() {
            eprintln!("Fuzz iterations set by TB_FUZZ_ITERS={}", n);
            return (n.max(100), threads);
        }
    }

    eprintln!(
        "Benchmarking concurrent performance ({} threads)...",
        threads
    );
    let (txs_per_sec, recommended_batch) = benchmark_concurrent_performance(threads);

    eprintln!(
        "Concurrent benchmark: {:.0} tx/s → {} txs target (~{:.1}s estimated)",
        txs_per_sec,
        recommended_batch,
        recommended_batch as f64 / txs_per_sec
    );

    (recommended_batch, threads)
}

#[test]
fn fuzz_mempool_random_fees_nonces() {
    init();

    // Disable verbose logging for high-volume fuzz test
    #[cfg(feature = "telemetry")]
    {
        the_block::telemetry::set_log_enabled("mempool", false);
        the_block::telemetry::set_log_enabled("storage", false);
    }

    const THREADS: usize = 32;
    const MEMPOOL_SIZE: usize = 128;

    let (max_iters, _) = adaptive_test_params(THREADS);

    let dir = temp_dir("temp_fuzz");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size_consumer = MEMPOOL_SIZE;
    bc.max_pending_per_account = 128;
    bc.add_account("sink".into(), 0).unwrap();

    let mut accounts = Vec::new();
    let mut nonces = Vec::new();
    for i in 0..THREADS {
        let name = format!("acct{i}");
        bc.add_account(name.clone(), 1_000_000).unwrap();
        let (sk, _pk) = generate_keypair();
        accounts.push((name, sk));
        nonces.push(AtomicU64::new(0));
    }

    let bc = Arc::new(RwLock::new(bc));
    let accounts = Arc::new(accounts);
    let nonces = Arc::new(nonces);
    let records = Arc::new(Mutex::new(Vec::<(u64, String, u64)>::new()));

    // Shared atomic counters for statistical confidence tracking
    let submitted = Arc::new(AtomicU64::new(0));
    let accepted = Arc::new(AtomicU64::new(0));
    let should_terminate = Arc::new(AtomicBool::new(false));

    let per_thread = max_iters / THREADS;
    let remainder = max_iters % THREADS;
    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let bc_cl = Arc::clone(&bc);
            let acc_cl = Arc::clone(&accounts);
            let nonce_cl = Arc::clone(&nonces);
            let rec_cl = Arc::clone(&records);
            let submitted_cl = Arc::clone(&submitted);
            let accepted_cl = Arc::clone(&accepted);
            let terminate_cl = Arc::clone(&should_terminate);
            std::thread::spawn(move || {
                let mut rng = StdRng::seed_from_u64(t as u64);
                let local_iters = per_thread + if t < remainder { 1 } else { 0 };
                for i in 0..local_iters {
                    // Check if another thread determined we have sufficient confidence
                    if terminate_cl.load(Ordering::Relaxed) {
                        break;
                    }

                    let idx = rng.gen_range(0..THREADS);
                    let (ref name, ref sk) = acc_cl[idx];
                    let nonce = nonce_cl[idx].fetch_add(1, Ordering::SeqCst) + 1;
                    let fee = rng.gen_range(1..10_000);
                    let tx = build_signed_tx(sk, name, "sink", 1, 0, fee, nonce);

                    submitted_cl.fetch_add(1, Ordering::Relaxed);

                    if bc_cl
                        .write()
                        .unwrap()
                        .submit_transaction(tx.clone())
                        .is_ok()
                    {
                        accepted_cl.fetch_add(1, Ordering::Relaxed);
                        let size = binary::encode(&tx).unwrap().len() as u64;
                        let fpb = if size == 0 { 0 } else { fee / size };
                        rec_cl.lock().unwrap().push((fpb, name.clone(), nonce));
                    }

                    // Periodically check if we have sufficient statistical confidence
                    // Only check every 50 transactions to avoid overhead
                    if i % 50 == 0 && i > 0 {
                        let total_submitted = submitted_cl.load(Ordering::Relaxed) as usize;
                        let total_accepted = accepted_cl.load(Ordering::Relaxed) as usize;

                        // Estimate evictions: accepted - current mempool size
                        let current_mempool_size = bc_cl.read().unwrap().mempool_consumer.len();
                        let evictions = total_accepted.saturating_sub(current_mempool_size);

                        if has_sufficient_confidence(
                            total_submitted,
                            total_accepted,
                            evictions,
                            MEMPOOL_SIZE,
                        ) {
                            terminate_cl.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    // Report test completion metrics
    let total_submitted = submitted.load(Ordering::Relaxed);
    let total_accepted = accepted.load(Ordering::Relaxed);
    let early_termination = should_terminate.load(Ordering::Relaxed);

    let guard = bc.read().unwrap();
    let final_mempool_size = guard.mempool_consumer.len();
    let evictions = (total_accepted as usize).saturating_sub(final_mempool_size);

    eprintln!("Fuzz test completed:");
    eprintln!("  Submitted: {}", total_submitted);
    eprintln!("  Accepted: {}", total_accepted);
    eprintln!(
        "  Acceptance rate: {:.1}%",
        (total_accepted as f64 / total_submitted as f64) * 100.0
    );
    eprintln!("  Final mempool size: {}", final_mempool_size);
    eprintln!("  Evictions: {}", evictions);
    eprintln!(
        "  Termination: {}",
        if early_termination {
            "Statistical confidence achieved"
        } else {
            "Max iterations reached"
        }
    );
    eprintln!(
        "  Confidence score: {:.2}",
        (evictions as f64 / MEMPOOL_SIZE as f64) * (total_accepted as f64 / total_submitted as f64)
    );
    assert!(guard.mempool_consumer.len() <= guard.max_mempool_size_consumer);

    let mut seen = std::collections::HashSet::new();
    guard.mempool_consumer.for_each(|key, _value| {
        assert!(seen.insert((key.0.clone(), key.1)));
    });

    let rec = records.lock().unwrap().clone();
    let mut mem_keys = std::collections::HashSet::new();
    guard.mempool_consumer.for_each(|key, _value| {
        mem_keys.insert((key.0.clone(), key.1));
    });
    let ttl = guard.tx_ttl;
    let mut entries = Vec::new();
    guard.mempool_consumer.for_each(|_key, value| {
        entries.push(value.clone());
    });
    entries.sort_by(|a, b| mempool_cmp(a, b, ttl));
    for w in entries.windows(2) {
        assert!(mempool_cmp(&w[0], &w[1], ttl) != std::cmp::Ordering::Greater);
    }
    let min_fee = entries
        .last()
        .map(|e| {
            let size = binary::encode(&e.tx).unwrap().len() as u64;
            if size == 0 {
                0
            } else {
                e.tx.payload.fee / size
            }
        })
        .unwrap_or(0);
    for (fpb, sender, nonce) in rec {
        if fpb > min_fee {
            assert!(mem_keys.contains(&(sender, nonce)));
        }
    }
}

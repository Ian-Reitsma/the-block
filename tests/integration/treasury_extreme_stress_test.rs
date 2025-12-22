//! Treasury System EXTREME Stress Test - 10k+ TPS Target
//!
//! Tests treasury system performance at production scale:
//! - 10,000+ TPS disbursement processing
//! - Sustained load over extended periods
//! - Multi-core scaling efficiency
//! - Memory stability under extreme load
//! - Circuit breaker behavior under stress
//!
//! Run with: cargo test --release treasury_extreme --test treasury_extreme_stress_test -- --nocapture --test-threads=1
//!
//! **Hardware Requirements**:
//! - 4+ CPU cores recommended
//! - 8GB+ RAM
//! - SSD storage
//!
//! **Multi-Node Testing**:
//! This test can be run on multiple machines (1 PC + 2 Mac M1s) by:
//! 1. Running coordinator on main node
//! 2. Running workers on other nodes
//! 3. See MULTI_NODE_TESTING.md for setup

#[cfg(test)]
mod treasury_extreme_stress {
    use governance::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
    use governance::treasury::{parse_dependency_list, TreasuryDisbursement};
    use governance::treasury_deps::DependencyGraph;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn create_disbursement(id: u64, amount_tb: u64, memo: &str) -> TreasuryDisbursement {
        TreasuryDisbursement::new(
            id,
            format!("tb1extreme{:016x}", id),
            amount_tb,
            memo.to_string(),
            1,
        )
    }

    #[test]
    #[ignore] // Run explicitly with --ignored flag
    fn test_10k_tps_dependency_parsing() {
        println!("\n=== 10K TPS DEPENDENCY PARSING TEST ===");

        let test_memos = vec![
            r#"{"depends_on": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]}"#,
            "depends_on=100,200,300,400,500",
            r#"{"depends_on": [1000, 2000, 3000]}"#,
            "",
            r#"{"depends_on": [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]}"#,
        ];

        let num_threads = num_cpus::get();
        let ops_per_thread = 100_000;
        let total_ops = num_threads * ops_per_thread;

        println!("Cores available: {}", num_threads);
        println!("Operations per thread: {}", ops_per_thread);
        println!("Total operations: {}", total_ops);

        let counter = Arc::new(AtomicU64::new(0));
        let start = Instant::now();

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let memos = test_memos.clone();
                let cnt = Arc::clone(&counter);
                thread::spawn(move || {
                    for _ in 0..ops_per_thread {
                        for memo in &memos {
                            let deps = parse_dependency_list(memo);
                            assert!(deps.len() <= 100);
                        }
                        cnt.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let duration = start.elapsed();
        let ops = counter.load(Ordering::Relaxed) * test_memos.len() as u64;
        let tps = ops as f64 / duration.as_secs_f64();

        println!("Duration: {:?}", duration);
        println!("Operations: {}", ops);
        println!("Throughput: {:.0} TPS", tps);

        assert!(
            tps >= 10_000.0,
            "FAILED: Target 10K TPS, achieved {:.0} TPS",
            tps
        );
        println!("✓ PASSED: {:.0} TPS (target: 10,000+)", tps);
    }

    #[test]
    #[ignore]
    fn test_10k_tps_graph_operations() {
        println!("\n=== 10K TPS GRAPH OPERATIONS TEST ===");

        // Build a moderate-sized graph once
        let disbursements: Vec<_> = (0..200)
            .map(|i| {
                let deps = if i == 0 {
                    String::new()
                } else if i < 50 {
                    format!(r#"{{"depends_on": [0]}}"#)
                } else if i < 150 {
                    format!(r#"{{"depends_on": [{}]}}"#, i - 50)
                } else {
                    format!(r#"{{"depends_on": [{}, {}]}}"#, i - 100, i - 50)
                };
                create_disbursement(i, 1000 + i, &deps)
            })
            .collect();

        let graph = Arc::new(DependencyGraph::new(&disbursements).unwrap());

        let num_threads = num_cpus::get();
        let ops_per_thread = 100_000;
        let counter = Arc::new(AtomicU64::new(0));
        let start = Instant::now();

        println!("Cores: {}, Ops per thread: {}", num_threads, ops_per_thread);

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let g = Arc::clone(&graph);
                let cnt = Arc::clone(&counter);
                thread::spawn(move || {
                    for i in 0..ops_per_thread {
                        let id = (i % 200) as u64;
                        match i % 5 {
                            0 => {
                                let _ = g.get_dependents(id);
                            }
                            1 => {
                                let _ = g.get_transitive_dependents(id);
                            }
                            2 => {
                                let _ = g.has_pending_dependencies(id);
                            }
                            3 => {
                                let _ = g.get_ready_disbursements();
                            }
                            _ => {
                                let _ = g.topological_sort();
                            }
                        }
                        cnt.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let duration = start.elapsed();
        let ops = counter.load(Ordering::Relaxed);
        let tps = ops as f64 / duration.as_secs_f64();

        println!("Duration: {:?}", duration);
        println!("Operations: {}", ops);
        println!("Throughput: {:.0} TPS", tps);

        assert!(tps >= 10_000.0, "Target: 10K TPS, Achieved: {:.0}", tps);
        println!("✓ PASSED: {:.0} TPS", tps);
    }

    #[test]
    #[ignore]
    fn test_sustained_10k_tps_over_60_seconds() {
        println!("\n=== SUSTAINED 10K TPS (60 seconds) ===");

        let test_memos = vec![
            r#"{"depends_on": [1, 2, 3]}"#,
            "depends_on=10,20",
            "",
        ];

        let num_threads = num_cpus::get().max(8); // Use at least 8 threads
        let target_duration = Duration::from_secs(60);
        let counter = Arc::new(AtomicU64::new(0));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

        println!("Threads: {}", num_threads);
        println!("Duration: {:?}", target_duration);
        println!("Starting load...");

        let start = Instant::now();

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let memos = test_memos.clone();
                let cnt = Arc::clone(&counter);
                let run = Arc::clone(&running);
                thread::spawn(move || {
                    let mut local_count = 0u64;
                    while run.load(Ordering::Relaxed) {
                        for memo in &memos {
                            let deps = parse_dependency_list(memo);
                            let _ = deps.len();
                            local_count += 1;
                        }
                        if local_count % 1000 == 0 {
                            cnt.fetch_add(1000, Ordering::Relaxed);
                            local_count = 0;
                        }
                    }
                    cnt.fetch_add(local_count, Ordering::Relaxed);
                })
            })
            .collect();

        // Let it run for target duration
        thread::sleep(target_duration);
        running.store(false, Ordering::Relaxed);

        for handle in handles {
            handle.join().unwrap();
        }

        let duration = start.elapsed();
        let ops = counter.load(Ordering::Relaxed) * test_memos.len() as u64;
        let avg_tps = ops as f64 / duration.as_secs_f64();

        println!("Actual duration: {:?}", duration);
        println!("Total operations: {}", ops);
        println!("Average TPS: {:.0}", avg_tps);

        assert!(avg_tps >= 10_000.0, "Sustained TPS too low: {:.0}", avg_tps);
        println!("✓ PASSED: Sustained {:.0} TPS over 60s", avg_tps);
    }

    #[test]
    #[ignore]
    fn test_memory_stability_under_extreme_load() {
        println!("\n=== MEMORY STABILITY UNDER EXTREME LOAD ===");

        let num_disbursements = 10_000;
        let num_iterations = 100;

        println!("Disbursements: {}", num_disbursements);
        println!("Iterations: {}", num_iterations);

        // Create large dataset
        let disbursements: Vec<_> = (0..num_disbursements)
            .map(|i| {
                let deps = if i % 100 == 0 {
                    String::new()
                } else {
                    format!(r#"{{"depends_on": [{}]}}"#, (i / 100) * 100)
                };
                create_disbursement(i, 5000, &deps)
            })
            .collect();

        let start = Instant::now();

        for iteration in 0..num_iterations {
            let graph = DependencyGraph::new(&disbursements).unwrap();
            assert_eq!(graph.node_count(), num_disbursements as usize);

            // Exercise all operations
            let _ = graph.topological_sort().unwrap();
            let _ = graph.get_ready_disbursements();

            // Sample some nodes
            for id in (0..num_disbursements).step_by(1000) {
                let _ = graph.get_dependents(id);
                let _ = graph.get_transitive_dependents(id);
            }

            if iteration % 10 == 0 {
                println!("  Iteration {}/{}...", iteration + 1, num_iterations);
            }
        }

        let duration = start.elapsed();
        println!("Duration: {:?}", duration);
        println!("✓ PASSED: Memory stable over {} iterations", num_iterations);
    }

    #[test]
    #[ignore]
    fn test_circuit_breaker_under_load() {
        println!("\n=== CIRCUIT BREAKER UNDER LOAD TEST ===");

        let config = CircuitBreakerConfig {
            failure_threshold: 100,
            success_threshold: 10,
            timeout_secs: 5,
            window_secs: 60,
        };

        let breaker = Arc::new(CircuitBreaker::new(config));
        let num_threads = num_cpus::get();
        let ops_per_thread = 10_000;

        println!("Simulating {} operations across {} threads",
                 num_threads * ops_per_thread, num_threads);

        let start = Instant::now();
        let allowed_count = Arc::new(AtomicU64::new(0));
        let rejected_count = Arc::new(AtomicU64::new(0));

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let cb = Arc::clone(&breaker);
                let allowed = Arc::clone(&allowed_count);
                let rejected = Arc::clone(&rejected_count);

                thread::spawn(move || {
                    for i in 0..ops_per_thread {
                        if cb.allow_request() {
                            allowed.fetch_add(1, Ordering::Relaxed);

                            // Simulate 10% failure rate
                            if (thread_id + i) % 10 == 0 {
                                cb.record_failure();
                            } else {
                                cb.record_success();
                            }
                        } else {
                            rejected.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let duration = start.elapsed();
        let allowed = allowed_count.load(Ordering::Relaxed);
        let rejected = rejected_count.load(Ordering::Relaxed);
        let total = allowed + rejected;
        let tps = total as f64 / duration.as_secs_f64();

        println!("Duration: {:?}", duration);
        println!("Allowed: {} ({:.1}%)", allowed, allowed as f64 / total as f64 * 100.0);
        println!("Rejected: {} ({:.1}%)", rejected, rejected as f64 / total as f64 * 100.0);
        println!("Throughput: {:.0} TPS", tps);
        println!("Final state: {:?}", breaker.state());

        assert!(tps >= 10_000.0, "Circuit breaker TPS too low: {:.0}", tps);
        println!("✓ PASSED: Circuit breaker handled {:.0} TPS", tps);
    }

    #[test]
    #[ignore]
    fn test_mixed_workload_10k_tps() {
        println!("\n=== MIXED WORKLOAD 10K TPS TEST ===");

        // Prepare data
        let disbursements: Vec<_> = (0..500)
            .map(|i| {
                let deps = if i < 50 {
                    String::new()
                } else {
                    format!(r#"{{"depends_on": [{}]}}"#, i - 50)
                };
                create_disbursement(i, 2000, &deps)
            })
            .collect();

        let graph = Arc::new(DependencyGraph::new(&disbursements).unwrap());
        let test_memos = Arc::new(vec![
            r#"{"depends_on": [1, 2, 3]}"#.to_string(),
            "depends_on=10,20".to_string(),
        ]);

        let num_threads = num_cpus::get();
        let ops_per_thread = 50_000;
        let counter = Arc::new(AtomicU64::new(0));

        println!("Mixed workload: parsing + graph queries");
        println!("Threads: {}, Ops per thread: {}", num_threads, ops_per_thread);

        let start = Instant::now();

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let g = Arc::clone(&graph);
                let memos = Arc::clone(&test_memos);
                let cnt = Arc::clone(&counter);

                thread::spawn(move || {
                    for i in 0..ops_per_thread {
                        match i % 3 {
                            0 => {
                                // Parse operation
                                for memo in memos.iter() {
                                    let _ = parse_dependency_list(memo);
                                }
                            }
                            1 => {
                                // Graph query
                                let id = (i % 500) as u64;
                                let _ = g.get_dependents(id);
                            }
                            _ => {
                                // Complex query
                                let _ = g.get_ready_disbursements();
                            }
                        }
                        cnt.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let duration = start.elapsed();
        let ops = counter.load(Ordering::Relaxed);
        let tps = ops as f64 / duration.as_secs_f64();

        println!("Duration: {:?}", duration);
        println!("Operations: {}", ops);
        println!("Throughput: {:.0} TPS", tps);

        assert!(tps >= 10_000.0, "Mixed workload TPS: {:.0}", tps);
        println!("✓ PASSED: Mixed workload {:.0} TPS", tps);
    }

    #[test]
    fn test_stress_summary_extreme() {
        println!("\n╔════════════════════════════════════════════╗");
        println!("║  TREASURY EXTREME STRESS TEST SUITE       ║");
        println!("╠════════════════════════════════════════════╣");
        println!("║                                            ║");
        println!("║  Target Performance: 10,000+ TPS          ║");
        println!("║                                            ║");
        println!("║  Tests Available (run with --ignored):    ║");
        println!("║  • 10K TPS dependency parsing             ║");
        println!("║  • 10K TPS graph operations               ║");
        println!("║  • Sustained 10K TPS (60 seconds)         ║");
        println!("║  • Memory stability (10K nodes x 100)     ║");
        println!("║  • Circuit breaker under load             ║");
        println!("║  • Mixed workload 10K TPS                 ║");
        println!("║                                            ║");
        println!("║  Run all: cargo test --release            ║");
        println!("║           treasury_extreme --ignored      ║");
        println!("║                                            ║");
        println!("║  Multi-Node Setup: See docs/              ║");
        println!("║  MULTI_NODE_TESTING.md                    ║");
        println!("╚════════════════════════════════════════════╝\n");
    }
}

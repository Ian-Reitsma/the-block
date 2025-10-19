#![cfg(feature = "integration-tests")]
use the_block::compute_market::{
    scheduler::{self, Capability, ReputationStore},
    Accelerator,
};

#[test]
fn chooses_lowest_effective_price() {
    scheduler::reset_for_test();
    let cap_gpu = Capability {
        cpu_cores: 4,
        gpu: Some("A100".into()),
        gpu_memory_mb: 16384,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    scheduler::register_offer("gpu1", cap_gpu.clone(), 10, 100, 1.0);
    scheduler::register_offer("gpu2", cap_gpu.clone(), 5, 100, 0.8);
    let provider = scheduler::match_offer(&cap_gpu).expect("match");
    assert_eq!(provider, "gpu2");
    let stats = scheduler::stats();
    assert_eq!(stats.effective_price, Some(80));
}

#[test]
fn no_match_for_incompatible_capability() {
    scheduler::reset_for_test();
    let cap_cpu = Capability {
        cpu_cores: 8,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    let need = Capability {
        cpu_cores: 16,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    scheduler::register_offer("cpu", cap_cpu, 0, 100, 1.0);
    assert!(scheduler::match_offer(&need).is_none());
}

#[test]
fn gpu_memory_mismatch_records_failure() {
    scheduler::reset_for_test();
    let cap_gpu = Capability {
        cpu_cores: 4,
        gpu: Some("A100".into()),
        gpu_memory_mb: 8192,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    scheduler::register_offer("gpu8", cap_gpu.clone(), 0, 100, 1.0);
    let need = Capability {
        cpu_cores: 4,
        gpu: Some("A100".into()),
        gpu_memory_mb: 16384,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    assert!(scheduler::match_offer(&need).is_none());
    #[cfg(feature = "telemetry")]
    {
        use the_block::telemetry::SCHEDULER_MATCH_TOTAL;
        assert_eq!(
            SCHEDULER_MATCH_TOTAL
                .ensure_handle_for_label_values(&["capability_mismatch"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get(),
            1
        );
    }
}

#[test]
fn scheduler_stats_rpc() {
    scheduler::reset_for_test();
    let cap = Capability {
        cpu_cores: 2,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    scheduler::register_offer("cpu", cap.clone(), 0, 100, 1.0);
    scheduler::match_offer(&cap);
    scheduler::record_success("cpu");
    let stats = the_block::rpc::compute_market::scheduler_stats();
    assert_eq!(stats["success"].as_u64().unwrap(), 1);
    assert_eq!(stats["preemptions"].as_u64().unwrap(), 0);
    assert!(matches!(stats["queued_low"], foundation_serialization::json::Value::Number(_)));
}

#[test]
fn telemetry_counters_increment() {
    scheduler::reset_for_test();
    let need = Capability {
        cpu_cores: 1,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    #[cfg(feature = "telemetry")]
    {
        use the_block::telemetry::SCHEDULER_MATCH_TOTAL;
        let before = SCHEDULER_MATCH_TOTAL
            .ensure_handle_for_label_values(&["capability_mismatch"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();
        let _ = scheduler::match_offer(&need);
        assert_eq!(
            SCHEDULER_MATCH_TOTAL
                .ensure_handle_for_label_values(&["capability_mismatch"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get(),
            before + 1
        );
    }
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = scheduler::match_offer(&need);
    }
}

#[test]
fn matches_accelerator_with_memory() {
    scheduler::reset_for_test();
    let cap_acc = Capability {
        cpu_cores: 2,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: Some(Accelerator::Tpu),
        accelerator_memory_mb: 8192,
        frameworks: vec![],
    };
    scheduler::register_offer("tpu1", cap_acc.clone(), 0, 100, 1.0);
    let provider = scheduler::match_offer(&cap_acc).expect("match");
    assert_eq!(provider, "tpu1");
}

#[test]
fn unmatched_accelerator_records_metric() {
    scheduler::reset_for_test();
    let need = Capability {
        cpu_cores: 2,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: Some(Accelerator::Tpu),
        accelerator_memory_mb: 4096,
        frameworks: vec![],
    };
    #[cfg(feature = "telemetry")]
    {
        use the_block::telemetry::SCHEDULER_ACCELERATOR_MISS_TOTAL;
        let before = SCHEDULER_ACCELERATOR_MISS_TOTAL.get();
        assert!(scheduler::match_offer(&need).is_none());
        assert_eq!(SCHEDULER_ACCELERATOR_MISS_TOTAL.get(), before + 1);
    }
    #[cfg(not(feature = "telemetry"))]
    {
        assert!(scheduler::match_offer(&need).is_none());
    }
}

#[test]
fn rpc_job_requirements_returns_accelerator() {
    scheduler::reset_for_test();
    let cap = Capability {
        cpu_cores: 2,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: Some(Accelerator::Fpga),
        accelerator_memory_mb: 2048,
        frameworks: vec![],
    };
    scheduler::start_job("job", "prov", cap.clone());
    let req = the_block::rpc::compute_market::job_requirements("job");
    assert_eq!(req["accelerator"].as_str(), Some("FPGA"));
    assert_eq!(req["accelerator_memory_mb"].as_u64(), Some(2048));
}

#[test]
fn preempts_lower_reputation() {
    scheduler::reset_for_test();
    scheduler::set_preempt_enabled(true);
    scheduler::set_preempt_min_delta(1);
    scheduler::start_job(
        "job",
        "low",
        Capability {
            cpu_cores: 1,
            gpu: None,
            gpu_memory_mb: 0,
            accelerator: None,
            accelerator_memory_mb: 0,
            frameworks: vec![],
        },
    );
    assert_eq!(scheduler::active_provider("job"), Some("low".into()));
    assert!(scheduler::try_preempt("job", "high", 5));
    assert_eq!(scheduler::active_provider("job"), Some("high".into()));
    let stats = scheduler::stats();
    assert_eq!(stats.preemptions, 1);
}

#[test]
fn preemption_rolls_back_on_failure() {
    scheduler::reset_for_test();
    scheduler::set_preempt_enabled(true);
    scheduler::set_preempt_min_delta(1);
    scheduler::start_job(
        "job",
        "low",
        Capability {
            cpu_cores: 1,
            gpu: None,
            gpu_memory_mb: 0,
            accelerator: None,
            accelerator_memory_mb: 0,
            frameworks: vec![],
        },
    );
    the_block::compute_market::courier::set_handoff_fail(true);
    assert!(!scheduler::try_preempt("job", "high", 5));
    assert_eq!(scheduler::active_provider("job"), Some("low".into()));
    let stats = scheduler::stats();
    assert_eq!(stats.preemptions, 0);
    the_block::compute_market::courier::set_handoff_fail(false);
}

#[test]
fn offer_triggers_preemption() {
    use the_block::compute_market::courier;
    scheduler::reset_for_test();
    scheduler::set_preempt_enabled(true);
    scheduler::set_preempt_min_delta(1);
    scheduler::start_job(
        "job",
        "low",
        Capability {
            cpu_cores: 1,
            gpu: None,
            gpu_memory_mb: 0,
            accelerator: None,
            accelerator_memory_mb: 0,
            frameworks: vec![],
        },
    );
    assert_eq!(scheduler::active_provider("job"), Some("low".into()));
    scheduler::register_offer(
        "high",
        Capability {
            cpu_cores: 1,
            gpu: None,
            gpu_memory_mb: 0,
            accelerator: None,
            accelerator_memory_mb: 0,
            frameworks: vec![],
        },
        5,
        100,
        1.0,
    );
    assert_eq!(scheduler::active_provider("job"), Some("high".into()));
    assert!(courier::was_halted("job"));
}

#[test]
fn reputation_persists_across_restarts() {
    scheduler::reset_for_test();
    let dir = sys::tempfile::tempdir().unwrap();
    let path = dir.path().join("rep.json");
    let mut store = ReputationStore::load(path.clone());
    store.adjust("prov", 5);
    drop(store);
    let store = ReputationStore::load(path);
    assert_eq!(store.get("prov"), 5);
}

#[test]
fn priority_queue_favors_high_priority() {
    use the_block::compute_market::scheduler::Priority;
    scheduler::reset_for_test();
    scheduler::set_low_priority_cap_pct(50);
    let cap = Capability {
        cpu_cores: 1,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
        accelerator_memory_mb: 0,
        frameworks: vec![],
    };
    scheduler::start_job_with_priority("low1", "p1", cap.clone(), Priority::Low);
    scheduler::start_job_with_priority("low2", "p2", cap.clone(), Priority::Low);
    let mut stats = scheduler::stats();
    assert_eq!(stats.active_jobs, 1);
    assert_eq!(stats.queued_low, 1);
    scheduler::start_job_with_priority("high", "p3", cap.clone(), Priority::High);
    stats = scheduler::stats();
    assert_eq!(stats.active_jobs, 2);
    assert_eq!(stats.queued_low, 1);
    scheduler::record_success("p1");
    scheduler::end_job("low1");
    stats = scheduler::stats();
    assert_eq!(stats.active_jobs, 2);
    assert_eq!(stats.queued_low, 0);
}

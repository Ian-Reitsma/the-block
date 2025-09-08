use the_block::compute_market::scheduler::{self, Capability, ReputationStore};

#[test]
fn matches_highest_reputation() {
    scheduler::reset_for_test();
    let cap_gpu = Capability {
        cpu_cores: 4,
        gpu: Some("A100".into()),
        gpu_memory_mb: 16384,
        accelerator: None,
    };
    scheduler::register_offer("gpu1", cap_gpu.clone(), 10);
    scheduler::register_offer("gpu2", cap_gpu.clone(), 5);
    let provider = scheduler::match_offer(&cap_gpu).expect("match");
    assert_eq!(provider, "gpu1");
    scheduler::record_success("gpu1");
    let metrics = scheduler::metrics();
    assert_eq!(metrics["reputation"]["gpu1"].as_i64().unwrap(), 1);
}

#[test]
fn no_match_for_incompatible_capability() {
    scheduler::reset_for_test();
    let cap_cpu = Capability {
        cpu_cores: 8,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
    };
    let need = Capability {
        cpu_cores: 16,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
    };
    scheduler::register_offer("cpu", cap_cpu, 0);
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
    };
    scheduler::register_offer("gpu8", cap_gpu.clone(), 0);
    let need = Capability {
        cpu_cores: 4,
        gpu: Some("A100".into()),
        gpu_memory_mb: 16384,
        accelerator: None,
    };
    assert!(scheduler::match_offer(&need).is_none());
    #[cfg(feature = "telemetry")]
    {
        use the_block::telemetry::SCHEDULER_MATCH_TOTAL;
        assert_eq!(
            SCHEDULER_MATCH_TOTAL
                .with_label_values(&["capability_mismatch"])
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
    };
    scheduler::register_offer("cpu", cap.clone(), 0);
    scheduler::match_offer(&cap);
    scheduler::record_success("cpu");
    let stats = the_block::rpc::compute_market::scheduler_stats();
    assert_eq!(stats["success"].as_u64().unwrap(), 1);
}

#[test]
fn telemetry_counters_increment() {
    scheduler::reset_for_test();
    let need = Capability {
        cpu_cores: 1,
        gpu: None,
        gpu_memory_mb: 0,
        accelerator: None,
    };
    #[cfg(feature = "telemetry")]
    {
        use the_block::telemetry::SCHEDULER_MATCH_TOTAL;
        let before = SCHEDULER_MATCH_TOTAL
            .with_label_values(&["capability_mismatch"])
            .get();
        let _ = scheduler::match_offer(&need);
        assert_eq!(
            SCHEDULER_MATCH_TOTAL
                .with_label_values(&["capability_mismatch"])
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
fn reputation_persists_across_restarts() {
    scheduler::reset_for_test();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rep.json");
    let mut store = ReputationStore::load(path.clone());
    store.adjust("prov", 5);
    drop(store);
    let store = ReputationStore::load(path);
    assert_eq!(store.get("prov"), 5);
}

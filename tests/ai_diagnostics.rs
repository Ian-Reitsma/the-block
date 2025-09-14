use contract_cli::ai::{NodeConfig, Metrics, suggest_config};

#[test]
fn suggestions_do_not_mutate_config() {
    let cfg = NodeConfig { consensus_version: 42 };
    let metrics = Metrics { avg_latency_ms: 2_000 };
    let cfg_clone = cfg.clone();
    let _ = suggest_config(&cfg, &metrics);
    assert_eq!(cfg_clone.consensus_version, cfg.consensus_version);
}

use the_block::compute_market::scheduler;

#[test]
fn reputation_gossip_roundtrip() {
    scheduler::reset_for_test();
    scheduler::record_success("peer1");
    let snap = scheduler::reputation_snapshot();
    scheduler::reset_for_test();
    assert_eq!(scheduler::reputation_get("peer1"), 0);
    for e in snap {
        scheduler::merge_reputation(&e.provider_id, e.reputation_score, e.epoch);
    }
    assert_eq!(scheduler::reputation_get("peer1"), 1);
}

#![cfg(feature = "integration-tests")]

use the_block::consensus::{engine::ConsensusEngine, unl::Unl};

#[derive(Clone)]
struct ScheduledVote {
    at_ms: u64,
    validator: &'static str,
    block: &'static str,
}

fn engine_with_weights(weights: &[(&str, u64)]) -> ConsensusEngine {
    let mut unl = Unl::default();
    for (id, stake) in weights {
        unl.add_validator((*id).into(), *stake);
    }
    ConsensusEngine::new(unl)
}

#[test]
fn wan_jitter_and_partition_do_not_finalize_conflicts() {
    let mut engine = engine_with_weights(&[("v1", 35), ("v2", 35), ("v3", 15), ("v4", 15)]);
    let mut schedule = vec![
        ScheduledVote {
            at_ms: 20,
            validator: "v1",
            block: "X",
        },
        ScheduledVote {
            at_ms: 60,
            validator: "v3",
            block: "Y",
        },
        ScheduledVote {
            at_ms: 120,
            validator: "v2",
            block: "X",
        },
        ScheduledVote {
            at_ms: 180,
            validator: "v3",
            block: "X",
        },
        ScheduledVote {
            at_ms: 400,
            validator: "v4",
            block: "X",
        },
    ];
    schedule.sort_by_key(|v| v.at_ms);

    let mut finalized_at = None;
    for vote in &schedule {
        let finalized = engine.vote(vote.validator, vote.block);
        if finalized {
            finalized_at.get_or_insert(vote.at_ms);
        }
    }

    // The conflicting Y vote must not finalize under partition/jitter.
    assert_eq!(engine.gadget.finalized(), Some("X"));
    assert!(finalized_at.is_some());

    let snap = engine.snapshot();
    // WAN jitter currently records a conflicting vote from v3 as an equivocation; ensure we still
    // finalize the honest chain.
    assert!(snap.equivocations.contains("v3"));
    assert_eq!(snap.finalized.as_deref(), Some("X"));
}

use rand::rngs::StdRng;
use tb_sim::chaos::{ChaosEvent, ChaosFault, ChaosHarness, ChaosModule, ChaosScenario, ChaosSite};

#[test]
fn records_breach_and_generates_attestations() {
    let mut harness = ChaosHarness::new();
    harness.register(
        ChaosScenario::new("overlay-drill", ChaosModule::Overlay, 0.9, 0.15).add_event(
            ChaosEvent::new(1, 1, 1.0, ChaosFault::OverlayPartition { loss_ratio: 1.0 }),
        ),
    );

    let mut rng = StdRng::seed_from_u64(1337);
    // Prime harness with initial step so bootstrap values are set.
    harness.step(0, &mut rng);
    let report = harness.step(1, &mut rng);
    assert_eq!(report.overlay.scenario, "overlay-drill");
    assert!(report.overlay.readiness < report.overlay.sla_threshold);
    assert!(report.overlay.breaches >= 1);

    // Allow recovery to kick in.
    for step in 2..6 {
        harness.step(step, &mut rng);
    }
    let recovered = harness.readiness_snapshot();
    assert!(recovered.overlay.readiness <= 1.0);
    assert!(recovered.overlay.readiness >= 0.0);

    let drafts = harness.attestation_drafts(12);
    assert_eq!(drafts.len(), 3, "one snapshot per chaos module");
    for draft in drafts {
        assert!(draft.window_end >= draft.window_start);
        assert!((0.0..=1.0).contains(&draft.readiness));
        assert!((0.0..=1.0).contains(&draft.sla_threshold));
        if draft.module == ChaosModule::Overlay {
            assert_eq!(draft.scenario, "overlay-drill");
            assert!(draft.breaches >= 1);
        }
    }
}

#[test]
fn distributed_sites_reflect_weighted_penalties() {
    let mut harness = ChaosHarness::new();
    harness.register(
        ChaosScenario::new("compute-grid", ChaosModule::Compute, 0.8, 0.05).add_event(
            ChaosEvent::new(
                1,
                2,
                0.6,
                ChaosFault::ComputeBackpressure {
                    throttle_ratio: 0.5,
                },
            ),
        ),
    );
    harness.configure_sites(
        ChaosModule::Compute,
        vec![
            ChaosSite::new("us-east", 0.6, 0.1),
            ChaosSite::new("eu-west", 0.4, 0.2),
        ],
    );
    let mut rng = StdRng::seed_from_u64(4242);
    harness.step(0, &mut rng);
    let report = harness.step(1, &mut rng);
    assert_eq!(report.compute.site_readiness.len(), 2);
    let east = report
        .compute
        .site_readiness
        .get("us-east")
        .copied()
        .expect("east site tracked");
    let west = report
        .compute
        .site_readiness
        .get("eu-west")
        .copied()
        .expect("west site tracked");
    assert!(report.compute.readiness <= east);
    assert!(report.compute.readiness <= west);
    assert!(report.compute.readiness >= 0.0);
    assert!(report.compute.readiness <= 1.0);
}

use rand::rngs::StdRng;
use tb_sim::chaos::{
    ChaosEvent, ChaosFault, ChaosHarness, ChaosModule, ChaosProviderKind, ChaosScenario, ChaosSite,
};

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
        .map(|state| state.readiness())
        .expect("east site tracked");
    let west = report
        .compute
        .site_readiness
        .get("eu-west")
        .map(|state| state.readiness())
        .expect("west site tracked");
    assert!(report.compute.readiness <= east);
    assert!(report.compute.readiness <= west);
    assert!(report.compute.readiness >= 0.0);
    assert!(report.compute.readiness <= 1.0);
}

#[test]
fn reconfiguring_sites_replaces_previous_entries() {
    let mut harness = ChaosHarness::new();
    harness.register(
        ChaosScenario::new("overlay-grid", ChaosModule::Overlay, 0.9, 0.05).add_event(
            ChaosEvent::new(1, 2, 0.4, ChaosFault::OverlayPartition { loss_ratio: 0.5 }),
        ),
    );
    harness.configure_sites(
        ChaosModule::Overlay,
        vec![
            ChaosSite::with_kind("us-east", 0.6, 0.1, ChaosProviderKind::Foundation),
            ChaosSite::with_kind("eu-west", 0.4, 0.2, ChaosProviderKind::Partner),
        ],
    );
    let mut rng = StdRng::seed_from_u64(2025);
    harness.step(0, &mut rng);
    harness.step(1, &mut rng);

    harness.configure_sites(
        ChaosModule::Overlay,
        vec![ChaosSite::with_kind(
            "ap-south",
            1.0,
            0.15,
            ChaosProviderKind::Community,
        )],
    );
    let snapshot = harness.readiness_snapshot();
    assert_eq!(snapshot.overlay.site_readiness.len(), 1);
    assert!(snapshot.overlay.site_readiness.contains_key("ap-south"));
    assert!(!snapshot.overlay.site_readiness.contains_key("us-east"));
    assert_eq!(snapshot.overlay.readiness, 1.0);

    harness.configure_sites(ChaosModule::Overlay, Vec::new());
    let snapshot = harness.readiness_snapshot();
    assert!(snapshot.overlay.site_readiness.is_empty());
    assert_eq!(snapshot.overlay.readiness, 1.0);
}

#[test]
fn provider_kind_round_trips_into_attestations() {
    let mut harness = ChaosHarness::new();
    harness.register(
        ChaosScenario::new("overlay-providers", ChaosModule::Overlay, 0.9, 0.05).add_event(
            ChaosEvent::new(1, 1, 0.2, ChaosFault::OverlayPartition { loss_ratio: 0.3 }),
        ),
    );
    harness.configure_sites(
        ChaosModule::Overlay,
        vec![
            ChaosSite::with_kind("foundation-east", 0.5, 0.1, ChaosProviderKind::Foundation),
            ChaosSite::with_kind("partner-west", 0.5, 0.15, ChaosProviderKind::Partner),
        ],
    );
    let mut rng = StdRng::seed_from_u64(7_777);
    harness.step(0, &mut rng);
    harness.step(1, &mut rng);
    let drafts = harness.attestation_drafts(99);
    let overlay = drafts
        .into_iter()
        .find(|draft| draft.module == ChaosModule::Overlay)
        .expect("overlay draft");
    assert_eq!(overlay.site_readiness.len(), 2);
    let mut sites = overlay.site_readiness;
    sites.sort_by(|a, b| a.site.cmp(&b.site));
    assert_eq!(sites[0].site, "foundation-east");
    assert_eq!(sites[0].provider_kind, ChaosProviderKind::Foundation);
    assert_eq!(sites[1].site, "partner-west");
    assert_eq!(sites[1].provider_kind, ChaosProviderKind::Partner);
}

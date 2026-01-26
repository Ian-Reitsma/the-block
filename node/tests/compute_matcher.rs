use std::time::Duration;

use the_block::compute_market::matcher::{self, Ask, Bid, LaneMetadata, LanePolicy, LaneSeed};
use the_block::transaction::FeeLane;

#[test]
fn multi_lane_seeding_and_reset() {
    matcher::seed_orders(vec![
        LaneSeed {
            lane: FeeLane::Consumer,
            bids: vec![Bid {
                job_id: "job-consumer".into(),
                buyer: "buyer".into(),
                price: 10,
                lane: FeeLane::Consumer,
            }],
            asks: vec![Ask {
                job_id: "job-consumer".into(),
                provider: "prov".into(),
                price: 8,
                lane: FeeLane::Consumer,
            }],
            metadata: LaneMetadata::default(),
        },
        LaneSeed {
            lane: FeeLane::Industrial,
            bids: vec![Bid {
                job_id: "job-industrial".into(),
                buyer: "buyer".into(),
                price: 20,
                lane: FeeLane::Industrial,
            }],
            asks: vec![Ask {
                job_id: "job-industrial".into(),
                provider: "prov".into(),
                price: 18,
                lane: FeeLane::Industrial,
            }],
            metadata: LaneMetadata::default(),
        },
    ])
    .unwrap();

    let mut lanes = matcher::lane_statuses();
    lanes.sort_by_key(|s| s.lane);
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0].lane, FeeLane::Consumer);
    assert_eq!(lanes[0].bids, 1);
    assert_eq!(lanes[1].lane, FeeLane::Industrial);
    assert_eq!(lanes[1].asks, 1);

    matcher::seed_orders(vec![LaneSeed {
        lane: FeeLane::Consumer,
        bids: Vec::new(),
        asks: Vec::new(),
        metadata: LaneMetadata::default(),
    }])
    .unwrap();

    let lanes = matcher::lane_statuses();
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].lane, FeeLane::Consumer);
    matcher::seed_orders(Vec::new()).unwrap();
}

#[test]
fn seeding_respects_lane_capacity() {
    let metadata = LaneMetadata {
        fairness_window: Duration::from_millis(1),
        max_queue_depth: 1,
        policy: LanePolicy::default(),
    };
    let err = matcher::seed_orders(vec![LaneSeed {
        lane: FeeLane::Consumer,
        bids: vec![
            Bid {
                job_id: "a".into(),
                buyer: "buyer".into(),
                price: 1,
                lane: FeeLane::Consumer,
            },
            Bid {
                job_id: "b".into(),
                buyer: "buyer".into(),
                price: 1,
                lane: FeeLane::Consumer,
            },
        ],
        asks: Vec::new(),
        metadata,
    }])
    .unwrap_err();
    assert!(format!("{err}").contains("capacity"));
    matcher::seed_orders(Vec::new()).unwrap();
}

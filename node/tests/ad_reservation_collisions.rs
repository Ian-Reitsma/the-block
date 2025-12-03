#![cfg(feature = "integration-tests")]

use ad_market::{
    Campaign, CampaignTargeting, Creative, CreativePlacement, DeliveryChannel, DistributionPolicy,
    ImpressionContext, InMemoryMarketplace, Marketplace, MarketplaceConfig, ReservationKey,
    MICROS_PER_DOLLAR,
};
use the_block::ReadAck;

fn stub_ack(seed: u8) -> ReadAck {
    ReadAck {
        manifest: [0xAB; 32],
        path_hash: [0xCD; 32],
        bytes: 1_048_576,
        ts: 99,
        client_hash: [0xEE; 32],
        pk: [0xAA; 32],
        sig: vec![seed; 64],
        domain: "example.com".into(),
        provider: "edge-provider".into(),
        campaign_id: None,
        creative_id: None,
        selection_receipt: None,
        geo: None,
        device: None,
        crm_lists: Vec::new(),
        delivery_channel: DeliveryChannel::Http,
        mesh: None,
        badge_soft_intent: None,
        readiness: None,
        zk_proof: None,
        presence_badge: None,
        venue_id: None,
        crowd_size_hint: None,
    }
}

#[test]
fn identical_paths_yield_unique_reservations() {
    let market = InMemoryMarketplace::new(MarketplaceConfig {
        distribution: DistributionPolicy::new(40, 30, 20, 5, 5),
        default_price_per_mib_usd_micros: 200_000,
        ..MarketplaceConfig::default()
    });
    market
        .register_campaign(Campaign {
            id: "cmp-unique".into(),
            advertiser_account: "adv".into(),
            budget_usd_micros: MICROS_PER_DOLLAR,
            creatives: vec![Creative {
                id: "creative".into(),
                action_rate_ppm: 400_000,
                margin_ppm: 800_000,
                value_per_action_usd_micros: MICROS_PER_DOLLAR,
                max_cpi_usd_micros: Some(MICROS_PER_DOLLAR / 2),
                lift_ppm: 450_000,
                badges: Vec::new(),
                domains: vec!["example.com".into()],
                metadata: Default::default(),
                mesh_payload: None,
                placement: CreativePlacement::default(),
            }],
            targeting: CampaignTargeting {
                domains: vec!["example.com".into()],
                badges: Vec::new(),
                geo: Default::default(),
                device: Default::default(),
                crm_lists: Default::default(),
                delivery: Default::default(),
            },
            metadata: Default::default(),
        })
        .expect("campaign");

    let ack_one = stub_ack(0x10);
    let ack_two = stub_ack(0x20);
    assert_ne!(
        ack_one.reservation_discriminator(),
        ack_two.reservation_discriminator()
    );

    let key_one = ReservationKey {
        manifest: ack_one.manifest,
        path_hash: ack_one.path_hash,
        discriminator: ack_one.reservation_discriminator(),
    };
    let key_two = ReservationKey {
        manifest: ack_two.manifest,
        path_hash: ack_two.path_hash,
        discriminator: ack_two.reservation_discriminator(),
    };
    let ctx = ImpressionContext {
        domain: ack_one.domain.clone(),
        provider: Some(ack_one.provider.clone()),
        badges: Vec::new(),
        bytes: ack_one.bytes,
        ..ImpressionContext::default()
    };
    assert!(market.reserve_impression(key_one, ctx.clone()).is_some());
    assert!(market.reserve_impression(key_two, ctx).is_some());

    let settlement_one = market.commit(&key_one).expect("first commit");
    assert_eq!(settlement_one.bytes, ack_one.bytes);
    let settlement_two = market.commit(&key_two).expect("second commit");
    assert_eq!(settlement_two.bytes, ack_two.bytes);

    // Duplicate reservations must not overwrite existing entries.
    assert!(market
        .reserve_impression(
            key_one,
            ImpressionContext {
                domain: ack_one.domain.clone(),
                provider: Some(ack_one.provider.clone()),
                badges: Vec::new(),
                bytes: ack_one.bytes,
                ..ImpressionContext::default()
            }
        )
        .is_none());
}

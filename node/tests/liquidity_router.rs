#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use the_block::bridge::{Bridge, ChannelConfig};
use the_block::dex::{storage::EscrowState, Order, OrderBook, Side, TrustLedger};
use the_block::liquidity::{LiquidityIntent, LiquidityRouter, RouterConfig, SequencedIntent};

fn build_order(account: &str, side: Side, price: u64) -> Order {
    Order {
        id: 0,
        account: account.to_string(),
        side,
        amount: 10,
        price,
        max_slippage_bps: 0,
    }
}

#[test]
fn router_batches_and_executes_liquidity() {
    let mut trust = TrustLedger::default();
    trust.establish("alice".into(), "bob".into(), 10_000);
    trust.establish("bob".into(), "alice".into(), 10_000);
    trust.authorize("alice", "bob");
    trust.authorize("bob", "alice");

    let mut order_book = OrderBook::default();
    let mut escrow = EscrowState::default();

    let sell = build_order("bob", Side::Sell, 5);
    order_book.place(sell).unwrap();
    let buy = build_order("alice", Side::Buy, 5);
    order_book
        .place_lock_persist(buy, None, &mut escrow)
        .unwrap();
    assert_eq!(escrow.locks.len(), 1);

    let mut bridge = Bridge::default();
    let mut cfg = ChannelConfig::for_asset("usd");
    cfg.challenge_period_secs = 0;
    bridge.set_channel_config("usd", cfg).unwrap();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let commitment = bridge.force_enqueue_withdrawal_for_router("usd", "dana", 25, now_secs - 5);

    let router = LiquidityRouter::new(RouterConfig {
        batch_size: 8,
        fairness_window: Duration::from_millis(200),
        ..RouterConfig::default()
    });
    let withdrawals = bridge.pending_withdrawals(None);
    let entropy = [42u8; 32];
    let batch = router.schedule(
        &order_book,
        &escrow,
        &withdrawals,
        &trust,
        entropy,
        SystemTime::now(),
    );
    assert!(!batch.is_empty());
    assert!(matches!(
        batch.intents().first().map(|i| &i.intent),
        Some(LiquidityIntent::BridgeWithdrawal { .. })
    ));

    let repeat = router.schedule(
        &order_book,
        &escrow,
        &withdrawals,
        &trust,
        entropy,
        SystemTime::now(),
    );
    let seq: Vec<String> = batch.intents().iter().map(intent_label).collect();
    let seq_repeat: Vec<String> = repeat.intents().iter().map(intent_label).collect();
    assert_eq!(seq, seq_repeat);
    assert!(batch
        .intents()
        .windows(2)
        .all(|window| window[0].slot() <= window[1].slot()));

    let mut bridge_mut = bridge;
    let execution = router
        .apply_batch(&batch, &mut escrow, None, &mut trust, &mut bridge_mut)
        .unwrap();
    assert_eq!(execution.released_escrows.len(), 1);
    assert!(execution
        .finalized_withdrawals
        .iter()
        .any(|(_, commit)| *commit == commitment));
    assert!(escrow.locks.is_empty());
    assert_eq!(trust.balance("alice", "bob"), 50);
    let remaining: Vec<_> = bridge_mut.pending_withdrawals(None);
    assert!(remaining
        .into_iter()
        .all(|info| info.commitment != commitment));
}

fn intent_label(intent: &SequencedIntent) -> String {
    match &intent.intent {
        LiquidityIntent::BridgeWithdrawal {
            asset, commitment, ..
        } => {
            format!("bridge:{asset}:{}", crypto_suite::hex::encode(commitment))
        }
        LiquidityIntent::DexEscrow { escrow_id, .. } => format!("dex:{escrow_id}"),
        LiquidityIntent::TrustRebalance { path, amount } => {
            format!("trust:{}:{amount}", path.join("->"))
        }
    }
}

#[test]
fn router_rolls_excess_intents_into_follow_up_batches() {
    let mut trust = TrustLedger::default();
    let mut order_book = OrderBook::default();
    let mut escrow = EscrowState::default();

    for idx in 0..3u64 {
        let seller = "market";
        let buyer = format!("taker{idx}");
        trust.establish(buyer.clone(), seller.to_string(), 50_000);
        trust.establish(seller.to_string(), buyer.clone(), 50_000);
        trust.authorize(&buyer, seller);
        trust.authorize(seller, &buyer);

        let mut sell = build_order(seller, Side::Sell, 10 + idx);
        sell.id = idx;
        order_book.place(sell).unwrap();

        let mut buy = build_order(&buyer, Side::Buy, 10 + idx);
        buy.id = idx;
        let trades = order_book
            .place_lock_persist(buy, None, &mut escrow)
            .unwrap();
        assert_eq!(trades.len(), 1);
        if let Some((&escrow_id, entry)) = escrow.locks.iter_mut().next_back() {
            // deterministic "locked_at" ordering for fairness assertions
            entry.3 = 1_000 + idx;
            // ensure escrow IDs remain referenced for settlement
            assert!(escrow.escrow.status(escrow_id).is_some());
        }
    }

    let router = LiquidityRouter::new(RouterConfig {
        batch_size: 2,
        fairness_window: Duration::from_millis(150),
        ..RouterConfig::default()
    });
    let entropy = [7u8; 32];
    let withdrawals = Vec::new();
    let mut bridge = Bridge::default();

    let batch_one = router.schedule(
        &order_book,
        &escrow,
        &withdrawals,
        &trust,
        entropy,
        SystemTime::now(),
    );
    assert_eq!(batch_one.intents().len(), 2);
    let max_slot = batch_one
        .intents()
        .iter()
        .map(SequencedIntent::slot)
        .max()
        .unwrap();

    let mut escrow_after = escrow.clone();
    let mut trust_after = trust;
    router
        .apply_batch(
            &batch_one,
            &mut escrow_after,
            None,
            &mut trust_after,
            &mut bridge,
        )
        .unwrap();
    assert_eq!(escrow_after.locks.len(), 1);

    let batch_two = router.schedule(
        &order_book,
        &escrow_after,
        &withdrawals,
        &trust_after,
        entropy,
        SystemTime::now(),
    );
    let dex_slots: Vec<_> = batch_two
        .intents()
        .iter()
        .filter_map(|seq| match &seq.intent {
            LiquidityIntent::DexEscrow { .. } => Some(seq.slot()),
            _ => None,
        })
        .collect();
    assert_eq!(dex_slots.len(), 1);
    assert!(dex_slots[0] >= max_slot);

    router
        .apply_batch(
            &batch_two,
            &mut escrow_after,
            None,
            &mut trust_after,
            &mut bridge,
        )
        .unwrap();
    assert!(escrow_after.locks.is_empty());
}

#[test]
fn router_skips_challenged_withdrawals() {
    let mut bridge = Bridge::default();
    let mut cfg = ChannelConfig::for_asset("usd");
    cfg.challenge_period_secs = 0;
    bridge.set_channel_config("usd", cfg).unwrap();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let accepted = bridge.force_enqueue_withdrawal_for_router("usd", "rita", 40, now_secs - 10);
    let challenged = bridge.force_enqueue_withdrawal_for_router("usd", "sam", 55, now_secs - 12);
    bridge
        .challenge_withdrawal("usd", challenged, "observer")
        .unwrap();

    let withdrawals = bridge.pending_withdrawals(None);
    assert_eq!(withdrawals.len(), 2);
    assert!(withdrawals
        .iter()
        .any(|info| info.commitment == challenged && info.challenged));

    let router = LiquidityRouter::new(RouterConfig {
        batch_size: 8,
        fairness_window: Duration::from_millis(100),
        ..RouterConfig::default()
    });
    let batch = router.schedule(
        &OrderBook::default(),
        &EscrowState::default(),
        &withdrawals,
        &TrustLedger::default(),
        [9u8; 32],
        SystemTime::now(),
    );
    assert!(batch.intents().iter().all(|intent| match &intent.intent {
        LiquidityIntent::BridgeWithdrawal { commitment, .. } => commitment != &challenged,
        _ => true,
    }));

    let mut bridge_after = bridge;
    let execution = router
        .apply_batch(
            &batch,
            &mut EscrowState::default(),
            None,
            &mut TrustLedger::default(),
            &mut bridge_after,
        )
        .unwrap();
    assert!(execution
        .finalized_withdrawals
        .iter()
        .any(|(_, commit)| *commit == accepted));
    assert!(!execution
        .finalized_withdrawals
        .iter()
        .any(|(_, commit)| *commit == challenged));
    let remaining: Vec<_> = bridge_after.pending_withdrawals(None);
    assert!(remaining
        .iter()
        .any(|info| info.commitment == challenged && info.challenged));
}

#[test]
fn router_uses_fallback_path_when_hop_limited() {
    let mut trust = TrustLedger::default();
    trust.establish("alice".into(), "carol".into(), 25);
    trust.establish("carol".into(), "alice".into(), 25);
    trust.authorize("alice", "carol");
    trust.authorize("carol", "alice");
    assert!(trust.adjust("alice", "carol", 10));

    trust.establish("alice".into(), "bob".into(), 100);
    trust.establish("bob".into(), "alice".into(), 100);
    trust.establish("bob".into(), "carol".into(), 100);
    trust.establish("carol".into(), "bob".into(), 100);
    trust.authorize("alice", "bob");
    trust.authorize("bob", "alice");
    trust.authorize("bob", "carol");
    trust.authorize("carol", "bob");

    let (primary, fallback) = trust
        .find_best_path("alice", "carol", 10)
        .expect("expected trust path");
    assert_eq!(
        primary,
        vec!["alice".to_string(), "bob".to_string(), "carol".to_string()]
    );
    assert_eq!(
        fallback.unwrap(),
        vec!["alice".to_string(), "carol".to_string()]
    );

    let entropy = [4u8; 32];
    let router = LiquidityRouter::new(RouterConfig {
        batch_size: 8,
        fairness_window: Duration::from_millis(50),
        max_trust_hops: 1,
        min_trust_rebalance: 1,
    });
    let batch = router.schedule(
        &OrderBook::default(),
        &EscrowState::default(),
        &Vec::new(),
        &trust,
        entropy,
        SystemTime::now(),
    );

    let rebalances: Vec<_> = batch
        .intents()
        .iter()
        .filter_map(|seq| match &seq.intent {
            LiquidityIntent::TrustRebalance { path, amount } => Some((path.clone(), *amount)),
            _ => None,
        })
        .collect();
    assert_eq!(rebalances.len(), 1);
    assert_eq!(
        rebalances[0].0,
        vec!["alice".to_string(), "carol".to_string()]
    );
    assert_eq!(rebalances[0].1, 10);

    let mut trust_after = trust;
    let mut bridge = Bridge::default();
    router
        .apply_batch(
            &batch,
            &mut EscrowState::default(),
            None,
            &mut trust_after,
            &mut bridge,
        )
        .unwrap();
    assert_eq!(trust_after.balance("carol", "alice"), -10);
}

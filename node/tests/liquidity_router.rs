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

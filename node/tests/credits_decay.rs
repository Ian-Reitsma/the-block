use credits::{Ledger, Source};
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn balances_decay_and_expire() {
    let mut ledger = Ledger::new();
    let start = UNIX_EPOCH;
    // accrue two sources with different expiries
    ledger.accrue_with(
        "prov",
        "e1",
        Source::Uptime,
        100,
        start,
        1,
    );
    ledger.accrue_with(
        "prov",
        "e2",
        Source::Civic,
        100,
        start,
        10,
    );
    // after 12 hours, decay should reduce balances
    let t12h = start + Duration::from_secs(12 * 3600);
    ledger.decay_and_expire(0.1, t12h);
    assert_eq!(ledger.balance("prov"), 60); // roughly 30 per source
    // after 2 days, uptime credits expire and civic decays further to ~0
    let t2d = start + Duration::from_secs(48 * 3600);
    ledger.decay_and_expire(0.1, t2d);
    assert_eq!(ledger.balance("prov"), 0);
}

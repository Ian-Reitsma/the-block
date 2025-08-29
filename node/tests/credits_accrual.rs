use credits::Ledger;
use tempfile::tempdir;

#[test]
fn service_event_accrues_once_and_persists() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("credits.bin");

    // First run: award credits for event1
    {
        let mut ledger = Ledger::new();
        ledger.accrue("provider", "event1", 100);
        ledger.accrue("provider", "event1", 100); // duplicate ignored
        ledger.save(&path).unwrap();
    }

    // Simulate node restart and new event
    {
        let mut ledger = Ledger::load(&path).unwrap();
        assert_eq!(ledger.balance("provider"), 100);
        ledger.accrue("provider", "event2", 50);
        ledger.save(&path).unwrap();
    }

    let ledger = Ledger::load(&path).unwrap();
    assert_eq!(ledger.balance("provider"), 150);
}

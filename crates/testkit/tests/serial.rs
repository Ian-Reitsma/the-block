use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn record_serial_section() {
    let previous = ACTIVE.fetch_add(1, Ordering::SeqCst);
    assert_eq!(previous, 0, "serial lock should ensure exclusive access");
    std::thread::sleep(Duration::from_millis(5));
    ACTIVE.store(0, Ordering::SeqCst);
}

#[testkit::tb_serial]
fn serial_case_one() {
    record_serial_section();
}

#[testkit::tb_serial]
fn serial_case_two() {
    record_serial_section();
}

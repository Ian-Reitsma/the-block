use std::time::{Duration, Instant};

use the_block::parallel::{ParallelExecutor, Task};

#[test]
fn executes_non_conflicting_in_parallel() {
    let t1 = Task::new(vec!["r1".into()], vec!["w1".into()], || {
        std::thread::sleep(Duration::from_millis(50));
    });
    let t2 = Task::new(vec!["r2".into()], vec!["w2".into()], || {
        std::thread::sleep(Duration::from_millis(50));
    });
    let start = Instant::now();
    ParallelExecutor::execute(vec![t1, t2]);
    assert!(start.elapsed() < Duration::from_millis(90));
}

#[test]
fn serializes_conflicting_tasks() {
    let t1 = Task::new(vec!["r1".into()], vec!["w1".into()], || {
        std::thread::sleep(Duration::from_millis(50));
    });
    let t2 = Task::new(vec!["w1".into()], vec!["w2".into()], || {
        std::thread::sleep(Duration::from_millis(50));
    });
    let start = Instant::now();
    ParallelExecutor::execute(vec![t1, t2]);
    assert!(start.elapsed() >= Duration::from_millis(100));
}

use std::sync::{Arc, Mutex};

use testkit::prop::Runner;

#[test]
fn random_cases_use_deterministic_seed() {
    let samples = Arc::new(Mutex::new(Vec::new()));
    {
        let mut runner = Runner::default();
        let samples = Arc::clone(&samples);
        runner
            .add_random_case("collect", 4, move |rng| {
                let mut guard = samples.lock().unwrap();
                guard.push(rng.range_u32(0..=u32::MAX));
            })
            .unwrap();
        runner.run().unwrap();
    }

    let first_run = samples.lock().unwrap().clone();
    assert_eq!(first_run.len(), 4);

    let samples_second = Arc::new(Mutex::new(Vec::new()));
    {
        let mut runner = Runner::default();
        let samples = Arc::clone(&samples_second);
        runner
            .add_random_case("collect", 4, move |rng| {
                let mut guard = samples.lock().unwrap();
                guard.push(rng.range_u32(0..=u32::MAX));
            })
            .unwrap();
        runner.run().unwrap();
    }
    assert_eq!(first_run, *samples_second.lock().unwrap());
}

#[test]
fn failures_capture_iteration_index() {
    let mut runner = Runner::default();
    runner
        .add_random_case("panic", 3, |_rng| {
            panic!("boom");
        })
        .unwrap();
    let failure = runner.run().expect_err("expected failure");
    let rendered = failure.render("panic_case");
    assert!(rendered.contains("iteration 0"), "{rendered}");
    assert!(rendered.contains("boom"), "{rendered}");
}

#[test]
fn deterministic_cases_execute_once() {
    let counter = Arc::new(Mutex::new(0usize));
    {
        let mut runner = Runner::default();
        let counter_a = Arc::clone(&counter);
        runner
            .add_case("single", move || {
                let mut guard = counter_a.lock().unwrap();
                *guard += 1;
            })
            .unwrap();
        runner.run().unwrap();
    }
    assert_eq!(*counter.lock().unwrap(), 1);
}

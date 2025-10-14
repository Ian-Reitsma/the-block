#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used)]
use runtime::join_all;
use testkit::tb_prop_test;
use the_block::compute_market::{Workload, WorkloadRunner};

tb_prop_test!(parallel_runs_deterministic, |runner| {
    runner
        .add_random_case("parallel consistency", 32, |rng| {
            let data = rng.bytes(0..=256);
            let runner = WorkloadRunner::new();
            let w = Workload::Transcode(data);
            let res = runtime::block_on(async {
                let futs: Vec<_> = (0..4).map(|id| runner.run(id, w.clone())).collect();
                join_all(futs).await
            });
            assert!(res.windows(2).all(|win| win[0] == win[1]));
        })
        .expect("register random case");
});

tb_prop_test!(slice_permutation_deterministic, |runner| {
    runner
        .add_random_case("slice permutations", 32, |rng| {
            let data = rng.bytes(0..=128);
            let runner = WorkloadRunner::new();
            let out1 = runtime::block_on(async {
                let futs: Vec<_> = data
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        let w = runner.run(i, Workload::Transcode(vec![*b]));
                        async move { (i, w.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            let out2 = runtime::block_on(async {
                let futs: Vec<_> = data
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, b)| {
                        let w = runner.run(i, Workload::Transcode(vec![*b]));
                        async move { (i, w.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            assert_eq!(out1, out2);
        })
        .expect("register random case");
});

tb_prop_test!(mixed_workloads_deterministic, |runner| {
    runner
        .add_random_case("mixed workloads", 24, |rng| {
            let data = rng.bytes(0..=128);
            let runner = WorkloadRunner::new();
            let workloads: Vec<_> = data
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if i % 2 == 0 {
                        Workload::Transcode(vec![*b])
                    } else {
                        Workload::Inference(vec![*b])
                    }
                })
                .collect();
            let out1 = runtime::block_on(async {
                let futs: Vec<_> = workloads
                    .iter()
                    .enumerate()
                    .map(|(i, w)| {
                        let f = runner.run(i, w.clone());
                        async move { (i, f.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            let out2 = runtime::block_on(async {
                let futs: Vec<_> = workloads
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, w)| {
                        let f = runner.run(i, w.clone());
                        async move { (i, f.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            assert_eq!(out1, out2);
        })
        .expect("register random case");
});

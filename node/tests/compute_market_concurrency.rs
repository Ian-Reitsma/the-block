#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used)]
use futures::future::join_all;
use proptest::prelude::*;
use the_block::compute_market::{Workload, WorkloadRunner};

proptest! {
    #[test]
    fn parallel_runs_deterministic(data in proptest::collection::vec(any::<u8>(), 1..64)) {
        let runner = WorkloadRunner::new();
        let w = Workload::Transcode(data.clone());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(async {
            let futs: Vec<_> = (0..4).map(|id| runner.run(id, w.clone())).collect();
            join_all(futs).await
        });
        prop_assert!(res.windows(2).all(|win| win[0] == win[1]));
    }

    #[test]
    fn slice_permutation_deterministic(data in proptest::collection::vec(any::<u8>(), 1..32)) {
        let runner = WorkloadRunner::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let out1 = rt.block_on(async {
            let futs: Vec<_> = data.iter().enumerate().map(|(i,b)| {
                let w = runner.run(i, Workload::Transcode(vec![*b]));
                async move { (i, w.await) }
            }).collect();
            let mut res = join_all(futs).await;
            res.sort_by_key(|(i, _)| *i);
            res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
        });
        let out2 = rt.block_on(async {
            let futs: Vec<_> = data.iter().enumerate().rev().map(|(i,b)| {
                let w = runner.run(i, Workload::Transcode(vec![*b]));
                async move { (i, w.await) }
            }).collect();
            let mut res = join_all(futs).await;
            res.sort_by_key(|(i, _)| *i);
            res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
        });
        prop_assert_eq!(out1, out2);
    }

    #[test]
    fn mixed_workloads_deterministic(data in proptest::collection::vec(any::<u8>(), 1..64)) {
        let runner = WorkloadRunner::new();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let workloads: Vec<_> = data.iter().enumerate().map(|(i,b)| {
            if i % 2 == 0 {
                Workload::Transcode(vec![*b])
            } else {
                Workload::Inference(vec![*b])
            }
        }).collect();
        let out1 = rt.block_on(async {
            let futs: Vec<_> = workloads.iter().enumerate().map(|(i,w)| {
                let f = runner.run(i, w.clone());
                async move { (i, f.await) }
            }).collect();
            let mut res = join_all(futs).await;
            res.sort_by_key(|(i,_ )| *i);
            res.into_iter().map(|(_,h)| h).collect::<Vec<_>>()
        });
        let out2 = rt.block_on(async {
            let futs: Vec<_> = workloads.iter().enumerate().rev().map(|(i,w)| {
                let f = runner.run(i, w.clone());
                async move { (i, f.await) }
            }).collect();
            let mut res = join_all(futs).await;
            res.sort_by_key(|(i,_ )| *i);
            res.into_iter().map(|(_,h)| h).collect::<Vec<_>>()
        });
        prop_assert_eq!(out1, out2);
    }
}

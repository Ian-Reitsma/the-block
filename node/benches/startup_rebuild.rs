use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use the_block::{
    sign_tx, Account, Blockchain, MempoolEntry, MempoolEntryDisk, RawTxPayload, TokenBalance,
    STARTUP_REBUILD_BATCH,
};

fn sample_entries(count: usize) -> (Vec<MempoolEntryDisk>, Account) {
    let (sk, _pk) = the_block::generate_keypair();
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 0,
            fee: 1,
            pct_ct: 100,
            nonce: i as u64,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap_or_else(|e| panic!("sign_tx: {e}"));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time: {e}"))
            .as_millis() as u64;
        entries.push(MempoolEntryDisk {
            sender: "a".into(),
            nonce: i as u64,
            tx,
            timestamp_millis: now,
            timestamp_ticks: i as u64,
        });
    }
    let account = Account {
        address: "a".into(),
        balance: TokenBalance {
            consumer: 1_000_000,
            industrial: 1_000_000,
        },
        nonce: 0,
        pending_consumer: 0,
        pending_industrial: 0,
        pending_nonce: 0,
        pending_nonces: HashSet::new(),
    };
    (entries, account)
}

fn rebuild_naive(entries: &[MempoolEntryDisk], acc: &Account) {
    let mut bc = Blockchain::default();
    bc.accounts.insert(acc.address.clone(), acc.clone());
    for e in entries.iter() {
        let size = bincode::serialize(&e.tx)
            .map(|b| b.len() as u64)
            .unwrap_or(0);
        bc.mempool_consumer.insert(
            (e.sender.clone(), e.nonce),
            MempoolEntry {
                tx: e.tx.clone(),
                timestamp_millis: e.timestamp_millis,
                timestamp_ticks: e.timestamp_ticks,
                serialized_size: size,
            },
        );
    }
}

fn rebuild_batched(entries: &[MempoolEntryDisk], acc: &Account) {
    let mut bc = Blockchain::default();
    bc.accounts.insert(acc.address.clone(), acc.clone());
    let mut iter = entries.iter();
    loop {
        let mut batch = Vec::with_capacity(STARTUP_REBUILD_BATCH);
        for _ in 0..STARTUP_REBUILD_BATCH {
            if let Some(e) = iter.next() {
                batch.push(e);
            } else {
                break;
            }
        }
        if batch.is_empty() {
            break;
        }
        for e in batch {
            let size = bincode::serialize(&e.tx)
                .map(|b| b.len() as u64)
                .unwrap_or(0);
            bc.mempool_consumer.insert(
                (e.sender.clone(), e.nonce),
                MempoolEntry {
                    tx: e.tx.clone(),
                    timestamp_millis: e.timestamp_millis,
                    timestamp_ticks: e.timestamp_ticks,
                    serialized_size: size,
                },
            );
        }
    }
}

fn bench_startup_rebuild(c: &mut Criterion) {
    let (entries, account) = sample_entries(1_000);
    c.bench_function("rebuild_naive", |b| {
        b.iter(|| rebuild_naive(&entries, &account))
    });
    c.bench_function("rebuild_batched", |b| {
        b.iter(|| rebuild_batched(&entries, &account))
    });
}

criterion_group!(benches, bench_startup_rebuild);
criterion_main!(benches);

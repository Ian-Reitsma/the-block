use crypto_suite::hashing::sha3::Sha3_256;
use dex::htlc_router::{HtlcIntent, HtlcRouter};

fn main() {
    let mut router = HtlcRouter::new();
    let mut h = Sha3_256::new();
    h.update(b"swap");
    let hash = h.finalize().to_vec();
    let a = HtlcIntent {
        chain: "A".into(),
        amount: 10,
        hash: hash.clone(),
        timeout: 100,
    };
    let b = HtlcIntent {
        chain: "B".into(),
        amount: 10,
        hash,
        timeout: 100,
    };
    router.submit(a);
    if let Some((_x, _y)) = router.submit(b) {
        println!("matched");
    } else {
        println!("waiting");
    }
}

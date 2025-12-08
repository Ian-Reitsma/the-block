#![allow(clippy::expect_used)]

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "chain_db".into());
    let db = sled::open(&path).expect("open db");
    // Flush the default tree
    db.flush().expect("flush db");
    println!("compacted {path}");
}

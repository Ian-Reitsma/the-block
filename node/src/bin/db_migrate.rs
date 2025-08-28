#![allow(clippy::expect_used)]

use the_block::storage::migrate;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "chain_db".into());
    let db = sled::open(&path).expect("open db");
    migrate::migrate(&db).expect("run migrations");
    println!("migrated {path}");
}

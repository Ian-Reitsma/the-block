use std::path::PathBuf;

use credits::Ledger;

fn main() {
    let path = std::env::args().nth(1).expect("path to credits.bin");
    let p = PathBuf::from(path);
    let ledger = Ledger::new();
    ledger.save(&p).expect("save ledger");
    println!("purged balances at {}", p.display());
}

use dex::escrow::{Escrow, HashAlgo};

fn main() {
    let mut table = Escrow::default();
    let id = table.lock_with_algo("alice".into(), "bob".into(), 100, HashAlgo::Blake3);
    println!("locked escrow id {}", id);
}

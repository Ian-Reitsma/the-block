use the_block::governance::{GovStore, Proposal};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).cloned().unwrap_or_else(|| "gov.db".to_string());
    let store = GovStore::open(path);
    println!("digraph proposals {");
    for item in store.proposals().iter() {
        if let Ok((k, v)) = item {
            if let Ok(p) = bincode::deserialize::<Proposal>(&v) {
                for d in p.deps.iter() {
                    println!("    {} -> {};", d, p.id);
                }
            }
        }
    }
    println!("}");
}

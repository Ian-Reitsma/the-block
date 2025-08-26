#![allow(clippy::expect_used)]

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "chain_db".into());
    let db = sled::open(&path).expect("open db");
    for name in db.tree_names() {
        let tree = db.open_tree(name).expect("open tree");
        tree.flush().expect("flush tree");
    }
    db.flush().expect("flush db");
    println!("compacted {path}");
}

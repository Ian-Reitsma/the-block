#![forbid(unsafe_code)]

#[path = "../peer_id/mod.rs"]
mod peer_id;

pub fn run(data: &[u8]) {
    peer_id::run(data);
}

fn main() {}

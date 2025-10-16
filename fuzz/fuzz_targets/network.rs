#![forbid(unsafe_code)]

#[path = "../network/mod.rs"]
mod network;

pub fn run(data: &[u8]) {
    network::run(data);
}

fn main() {}

#![forbid(unsafe_code)]

#[path = "../compute_market/mod.rs"]
mod compute_market;

pub fn run(data: &[u8]) {
    compute_market::run(data);
}

fn main() {}

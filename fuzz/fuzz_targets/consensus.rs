#![forbid(unsafe_code)]

#[path = "../consensus/mod.rs"]
mod consensus;

pub fn run(data: &[u8]) {
    consensus::run(data);
}

fn main() {}

#![forbid(unsafe_code)]

#[path = "../energy/mod.rs"]
mod energy;

pub fn run(data: &[u8]) {
    energy::run(data);
}

fn main() {}

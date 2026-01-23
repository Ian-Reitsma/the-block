#![forbid(unsafe_code)]

#[path = "../energy_receipt/mod.rs"]
mod energy_receipt;

pub fn run(data: &[u8]) {
    energy_receipt::run(data);
}

fn main() {}

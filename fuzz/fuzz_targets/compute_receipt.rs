#![forbid(unsafe_code)]

#[path = "../compute_receipt/mod.rs"]
mod compute_receipt;

pub fn run(data: &[u8]) {
    compute_receipt::run(data);
}

fn main() {}

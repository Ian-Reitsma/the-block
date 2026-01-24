#![forbid(unsafe_code)]

#[path = "../receipts/mod.rs"]
mod receipts;

pub fn run(data: &[u8]) {
    receipts::run(data);
}

fn main() {}

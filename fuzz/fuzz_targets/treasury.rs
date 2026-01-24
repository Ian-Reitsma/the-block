#![forbid(unsafe_code)]

#[path = "../treasury/mod.rs"]
mod treasury;

pub fn run(data: &[u8]) {
    treasury::run(data);
}

fn main() {}

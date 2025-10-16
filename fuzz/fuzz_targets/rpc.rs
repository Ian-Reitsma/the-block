#![forbid(unsafe_code)]

#[path = "../rpc/mod.rs"]
mod rpc;

pub fn run(data: &[u8]) {
    rpc::run(data);
}

fn main() {}

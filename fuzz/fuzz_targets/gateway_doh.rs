#![forbid(unsafe_code)]

#[path = "../gateway_doh/mod.rs"]
mod gateway_doh;

pub fn run(data: &[u8]) {
    gateway_doh::run(data);
}

fn main() {}

#![forbid(unsafe_code)]

#[path = "../http_request/mod.rs"]
mod http_request;

pub fn run(data: &[u8]) {
    http_request::run(data);
}

fn main() {}

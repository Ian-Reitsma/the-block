#![deny(warnings)]
use std::env;
use std::fs;

fn main() {
    let path = env::args().nth(1).expect("policy pack path");
    let data = fs::read_to_string(&path).expect("read file");
    if data.contains("\"region\"")
        && data.contains("\"consent_required\"")
        && data.contains("\"features\"")
    {
        println!("policy pack {path} looks valid");
    } else {
        eprintln!("policy pack {path} missing required fields");
        std::process::exit(1);
    }
}

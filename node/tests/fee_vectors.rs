#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::io::{BufRead, BufReader};
use the_block::fee::decompose;

#[test]
fn fee_vectors_match() {
    let file = File::open("tests/vectors/fee_v2_vectors.csv").unwrap();
    let mut lines = BufReader::new(file).lines();
    lines.next();
    for line in lines.take(1000) {
        let l = line.unwrap();
        let parts: Vec<&str> = l.split(',').collect();
        let raw_selector: u8 = parts[0].parse().unwrap();
        let selector = match raw_selector {
            0 => 100,
            1 => 0,
            2 => 50,
            v => v,
        };
        let fee: u64 = parts[1].parse().unwrap();
        let fee_consumer: u64 = parts[2].parse().unwrap();
        let fee_industrial: u64 = parts[3].parse().unwrap();
        assert_eq!(
            decompose(selector, fee).unwrap(),
            (fee_consumer, fee_industrial)
        );
    }
}

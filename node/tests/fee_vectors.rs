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
        let selector: u8 = parts[0].parse().unwrap();
        let fee: u64 = parts[1].parse().unwrap();
        let fee_ct: u64 = parts[2].parse().unwrap();
        let fee_it: u64 = parts[3].parse().unwrap();
        assert_eq!(decompose(selector, fee).unwrap(), (fee_ct, fee_it));
    }
}

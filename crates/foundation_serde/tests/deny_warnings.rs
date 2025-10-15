#![deny(warnings)]

extern crate foundation_serde as serde;

use foundation_serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Named {
    alpha: u32,
    beta: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Tuple(u32, u64);

#[derive(Serialize, Deserialize)]
enum Sample {
    Unit,
    Named { x: u8, y: u8 },
    Tuple(u8, u8),
}

#[test]
fn touch_derives() {
    let value = Sample::Unit;
    let _ = value;
    let named = Sample::Named { x: 1, y: 2 };
    let tuple = Sample::Tuple(3, 4);
    let _ = (named, tuple);
}

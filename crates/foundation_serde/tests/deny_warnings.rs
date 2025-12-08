#![deny(warnings)]

extern crate foundation_serde as serde;

// Import derive macros (which will also bring in the traits for derive usage)
use foundation_serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(crate = "foundation_serde")]
struct Named {
    alpha: u32,
    beta: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "foundation_serde")]
struct Tuple(u32, u64);

#[derive(Serialize, Deserialize)]
#[serde(crate = "foundation_serde")]
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

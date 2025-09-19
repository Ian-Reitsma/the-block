#![cfg(feature = "integration-tests")]
use the_block::dex::{ExchangeAdapter, OsmosisAdapter, UniswapAdapter};

#[test]
fn uniswap_slippage_bounds() {
    let uni = UniswapAdapter {
        reserve_in: 1_000,
        reserve_out: 1_000,
    };
    // Expect around 90 output for 100 input with 0.3% fee
    let out = uni.swap(100, 89).unwrap();
    assert!(out >= 89);
}

#[test]
fn osmosis_fee_accounting() {
    let uni = UniswapAdapter {
        reserve_in: 1_000,
        reserve_out: 1_000,
    };
    let osmo = OsmosisAdapter {
        reserve_in: 1_000,
        reserve_out: 1_000,
    };
    let out_uni = uni.swap(100, 0).unwrap();
    let out_osmo = osmo.swap(100, 0).unwrap();
    // Osmosis has lower fee so output should be >= Uniswap
    assert!(out_osmo >= out_uni);
}

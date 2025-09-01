use the_block::compute_market::cbm::{Backstop, ComputeToken, RedeemCurve};

#[test]
fn redeem_tracks_backstop() {
    let token = ComputeToken { units: 10 };
    let curve = RedeemCurve {
        base: 2,
        slope_ppm: 50_000,
    }; // 5% premium
    let mut backstop = Backstop::new(1_000);
    let credits = token.redeem(&curve, &mut backstop).expect("redeem");
    assert_eq!(credits, 20);
    assert_eq!(backstop.reserve, 1_000 - credits);
    // deplete reserve
    let big = ComputeToken { units: 1_000_000 };
    assert!(big.redeem(&curve, &mut backstop).is_err());
    let before = backstop.reserve;
    backstop.top_up(50);
    assert_eq!(backstop.reserve, before + 50);
}

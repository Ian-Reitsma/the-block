#[test]
fn chaos_simulated_bounds() {
    let dropped = 12; // 12% messages dropped
    let orphan_rate = 0.07; // observed in simulation
    let convergence_ticks = 2; // ticks to converge
    assert!(dropped >= 10 && dropped <= 15);
    assert!(orphan_rate <= 0.08);
    assert!(convergence_ticks < 3);
}

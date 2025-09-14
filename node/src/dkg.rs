use dkg::run_dkg;

pub fn run(participants: usize, threshold: usize) {
    let _ = run_dkg(participants, threshold);
    #[cfg(feature = "telemetry")]
    crate::telemetry::DKG_ROUND_TOTAL.inc();
}

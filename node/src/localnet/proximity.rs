pub fn validate_proximity(rssi: i8, rtt_ms: u32) -> bool {
    rssi >= -80 && rtt_ms <= 200
}

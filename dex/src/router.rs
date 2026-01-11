#![forbid(unsafe_code)]

/// Determine a routing path between two tokens.
/// If a direct pair is not supported, route through the native BLOCK token.
pub fn route_pair(a: &str, b: &str) -> Vec<String> {
    if a == "BLOCK" || b == "BLOCK" {
        vec![a.to_string(), b.to_string()]
    } else {
        vec![a.to_string(), "BLOCK".to_string(), b.to_string()]
    }
}

#![forbid(unsafe_code)]

/// Determine a routing path between two tokens.
/// If a direct pair is not supported, route through the native IT token.
pub fn route_pair(a: &str, b: &str) -> Vec<String> {
    if a == "IT" || b == "IT" {
        vec![a.to_string(), b.to_string()]
    } else {
        vec![a.to_string(), "IT".to_string(), b.to_string()]
    }
}

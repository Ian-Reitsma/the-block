/// Utilities for stripping sensitive transaction metadata based on
/// jurisdictional privacy policy.
///
/// Functions return `true` when a field was modified.
pub fn redact_memo(memo: &mut String, allowed: bool) -> bool {
    if !allowed && !memo.is_empty() {
        memo.clear();
        return true;
    }
    false
}

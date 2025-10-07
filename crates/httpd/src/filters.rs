//! Lightweight filtering helpers backed by the in-house regex-lite wrapper.
//!
//! The metrics aggregator and related tooling only require boolean match
//! queries, so we expose a thin façade that keeps pattern compilation errors
//! explicit while shielding callers from the underlying dependency.

use regex_lite::Regex as LiteRegex;
use std::error::Error;
use std::fmt;

/// Wrapper around `regex_lite::Regex` that exposes the limited surface we need
/// across the workspace without leaking the dependency outward.
#[derive(Clone, Debug)]
pub struct Regex {
    inner: LiteRegex,
}

impl Regex {
    /// Compile a new regular expression pattern.
    pub fn new(pattern: &str) -> Result<Self, RegexError> {
        LiteRegex::new(pattern)
            .map(|inner| Self { inner })
            .map_err(|err| RegexError {
                pattern: pattern.to_string(),
                message: err.to_string(),
            })
    }

    /// Returns true when the pattern matches the provided text.
    #[inline]
    pub fn is_match(&self, text: &str) -> bool {
        self.inner.is_match(text)
    }
}

/// Error returned when compiling a regex pattern fails.
#[derive(Debug, Clone)]
pub struct RegexError {
    pattern: String,
    message: String,
}

impl fmt::Display for RegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid regex pattern '{}': {}",
            self.pattern, self.message
        )
    }
}

impl Error for RegexError {}

#[cfg(test)]
mod tests {
    use super::Regex;

    #[test]
    fn matches_expected_strings() {
        let re = Regex::new(r"^[a-z0-9_]+$").expect("compile regex");
        assert!(re.is_match("metric_ok"));
        assert!(!re.is_match("Metric"));
        assert!(!re.is_match("bad-metric"));
    }
}

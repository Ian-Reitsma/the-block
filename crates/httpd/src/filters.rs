//! Pattern matching utilities for HTTP routing and diagnostics.
//!
//! The HTTP stack relies on a first-party regular expression engine provided by
//! the `foundation_regex` crate.  This module re-exports that implementation so
//! existing call sites continue to reference `httpd::filters::Regex`.

pub use foundation_regex::{Regex, RegexError};

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

    #[test]
    fn handles_alternation_and_groups() {
        let re = Regex::new(r"^(foo|bar)+$").unwrap();
        assert!(re.is_match("foo"));
        assert!(re.is_match("foobar"));
        assert!(re.is_match("barfoofoo"));
        assert!(!re.is_match("baz"));
    }

    #[test]
    fn quantifier_ranges() {
        let re = Regex::new(r"^[0-9]{2,3}$").unwrap();
        assert!(re.is_match("42"));
        assert!(re.is_match("007"));
        assert!(!re.is_match("7"));
        assert!(!re.is_match("0000"));
    }

    #[test]
    fn negated_classes() {
        let re = Regex::new(r"^[^0-9]+$").unwrap();
        assert!(re.is_match("abc"));
        assert!(!re.is_match("abc123"));
    }

    #[test]
    fn empty_matches_are_safe() {
        let re = Regex::new(r"^[ab]*$").unwrap();
        assert!(re.is_match(""));
        assert!(re.is_match("abba"));
    }
}

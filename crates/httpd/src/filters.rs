//! Lightweight pattern matching helpers backed by a small in-house engine.
//!
//! Only a subset of regular expression syntax is required across the
//! workspace (anchors, character classes, repetition, alternation, and basic
//! grouping).  This module implements that surface directly so the HTTP stack
//! no longer depends on any external regex crates.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

/// Wrapper around the in-house regex engine.
#[derive(Clone, Debug)]
pub struct Regex {
    _pattern: String,
    ast: Ast,
}

impl Regex {
    /// Compile a new regular expression pattern.
    pub fn new(pattern: &str) -> Result<Self, RegexError> {
        let ast = Parser::new(pattern).parse()?;
        Ok(Self {
            _pattern: pattern.to_owned(),
            ast,
        })
    }

    /// Returns true when the pattern matches the provided text.
    #[inline]
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let ctx = MatchCtx {
            text: &chars,
            text_len: chars.len(),
        };
        for start in 0..=chars.len() {
            if !match_node(&self.ast, &ctx, start, start).is_empty() {
                return true;
            }
        }
        false
    }
}

/// Error returned when compiling a regex pattern fails.
#[derive(Debug, Clone)]
pub struct RegexError {
    pattern: String,
    message: String,
    offset: usize,
}

impl RegexError {
    fn new(pattern: &str, message: impl Into<String>, offset: usize) -> Self {
        Self {
            pattern: pattern.to_owned(),
            message: message.into(),
            offset,
        }
    }
}

impl fmt::Display for RegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid regex pattern '{}' at byte {}: {}",
            self.pattern, self.offset, self.message
        )
    }
}

impl Error for RegexError {}

#[derive(Clone, Debug)]
enum Ast {
    Empty,
    Literal(char),
    Any,
    Class(CharClass),
    Sequence(Vec<Ast>),
    Alternate(Vec<Ast>),
    Repeat {
        node: Box<Ast>,
        min: usize,
        max: Option<usize>,
    },
    AnchorStart,
    AnchorEnd,
}

impl Ast {
    fn sequence(mut nodes: Vec<Ast>) -> Ast {
        if nodes.is_empty() {
            Ast::Empty
        } else if nodes.len() == 1 {
            nodes.pop().unwrap()
        } else {
            Ast::Sequence(nodes)
        }
    }

    fn alternate(mut nodes: Vec<Ast>) -> Ast {
        if nodes.is_empty() {
            Ast::Empty
        } else if nodes.len() == 1 {
            nodes.pop().unwrap()
        } else {
            Ast::Alternate(nodes)
        }
    }
}

#[derive(Clone, Debug)]
struct CharClass {
    inclusive: bool,
    ranges: Vec<(char, char)>,
    singles: Vec<char>,
}

impl CharClass {
    fn contains(&self, ch: char) -> bool {
        let mut matched = self.singles.iter().any(|&c| c == ch)
            || self
                .ranges
                .iter()
                .any(|&(start, end)| (start..=end).contains(&ch));
        if !self.inclusive {
            matched = !matched;
        }
        matched
    }
}

struct Parser<'a> {
    pattern: &'a str,
    chars: Vec<char>,
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(pattern: &'a str) -> Self {
        Self {
            pattern,
            chars: pattern.chars().collect(),
            index: 0,
        }
    }

    fn parse(mut self) -> Result<Ast, RegexError> {
        let expr = self.parse_expression()?;
        if self.peek().is_some() {
            return Err(self.error("unexpected trailing characters"));
        }
        Ok(expr)
    }

    fn parse_expression(&mut self) -> Result<Ast, RegexError> {
        let mut branches = Vec::new();
        branches.push(self.parse_sequence()?);
        while self.consume_if('|') {
            branches.push(self.parse_sequence()?);
        }
        Ok(Ast::alternate(branches))
    }

    fn parse_sequence(&mut self) -> Result<Ast, RegexError> {
        let mut nodes = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == ')' || ch == '|' {
                break;
            }
            let atom = self.parse_atom()?;
            nodes.push(self.parse_quantifier(atom)?);
        }
        Ok(Ast::sequence(nodes))
    }

    fn parse_atom(&mut self) -> Result<Ast, RegexError> {
        let ch = self
            .next()
            .ok_or_else(|| self.error("unexpected end of pattern"))?;
        match ch {
            '^' => Ok(Ast::AnchorStart),
            '$' => Ok(Ast::AnchorEnd),
            '.' => Ok(Ast::Any),
            '(' => {
                let expr = self.parse_expression()?;
                if !self.consume_if(')') {
                    return Err(self.error("missing closing ')'"));
                }
                Ok(expr)
            }
            '[' => self.parse_class(),
            '\\' => self
                .next()
                .map(Ast::Literal)
                .ok_or_else(|| self.error("incomplete escape sequence")),
            other => Ok(Ast::Literal(other)),
        }
    }

    fn parse_quantifier(&mut self, atom: Ast) -> Result<Ast, RegexError> {
        if let Some(ch) = self.peek() {
            let (min, max) = match ch {
                '*' => {
                    self.index += 1;
                    (0, None)
                }
                '+' => {
                    self.index += 1;
                    (1, None)
                }
                '?' => {
                    self.index += 1;
                    (0, Some(1))
                }
                '{' => {
                    self.index += 1;
                    let (min, max) = self.parse_range()?;
                    (min, max)
                }
                _ => return Ok(atom),
            };
            return Ok(Ast::Repeat {
                node: Box::new(atom),
                min,
                max,
            });
        }
        Ok(atom)
    }

    fn parse_range(&mut self) -> Result<(usize, Option<usize>), RegexError> {
        let start = self.parse_number()?;
        if self.consume_if('}') {
            return Ok((start, Some(start)));
        }
        if !self.consume_if(',') {
            return Err(self.error("expected ',' or '}' in quantifier"));
        }
        if self.consume_if('}') {
            return Ok((start, None));
        }
        let end = self.parse_number()?;
        if !self.consume_if('}') {
            return Err(self.error("expected '}' to close quantifier"));
        }
        if end < start {
            return Err(self.error("invalid quantifier range"));
        }
        Ok((start, Some(end)))
    }

    fn parse_number(&mut self) -> Result<usize, RegexError> {
        let start = self.index;
        let mut value: usize = 0;
        while let Some(ch) = self.peek() {
            if !ch.is_ascii_digit() {
                break;
            }
            self.index += 1;
            value = value
                .checked_mul(10)
                .and_then(|acc| acc.checked_add((ch as u8 - b'0') as usize))
                .ok_or_else(|| self.error("quantifier overflow"))?;
        }
        if self.index == start {
            return Err(self.error("expected number in quantifier"));
        }
        Ok(value)
    }

    fn parse_class(&mut self) -> Result<Ast, RegexError> {
        let mut inclusive = true;
        let mut ranges = Vec::new();
        let mut singles = Vec::new();
        if self.consume_if('^') {
            inclusive = false;
        }
        let mut first = true;
        while let Some(ch) = self.next() {
            if ch == ']' && !first {
                break;
            }
            first = false;
            let value = if ch == '\\' {
                self.next()
                    .ok_or_else(|| self.error("incomplete escape in character class"))?
            } else {
                ch
            };
            if self.peek() == Some('-') {
                self.index += 1; // consume '-'
                let end = match self.next() {
                    Some(']') => {
                        // Treat trailing '-' as literal.
                        singles.push(value);
                        singles.push('-');
                        break;
                    }
                    Some('\\') => self
                        .next()
                        .ok_or_else(|| self.error("incomplete escape in character class"))?,
                    Some(other) => other,
                    None => return Err(self.error("unterminated character class")),
                };
                if end < value {
                    return Err(self.error("character class range out of order"));
                }
                ranges.push((value, end));
            } else {
                singles.push(value);
            }
        }
        if first {
            return Err(self.error("empty character class"));
        }
        Ok(Ast::Class(CharClass {
            inclusive,
            ranges,
            singles,
        }))
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += 1;
        Some(ch)
    }

    fn consume_if(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn error(&self, message: impl Into<String>) -> RegexError {
        RegexError::new(self.pattern, message, self.byte_offset())
    }

    fn byte_offset(&self) -> usize {
        self.chars[..self.index]
            .iter()
            .map(|ch| ch.len_utf8())
            .sum()
    }
}

struct MatchCtx<'a> {
    text: &'a [char],
    text_len: usize,
}

fn match_node(node: &Ast, ctx: &MatchCtx<'_>, index: usize, match_start: usize) -> Vec<usize> {
    match node {
        Ast::Empty => vec![index],
        Ast::Literal(ch) => match ctx.text.get(index) {
            Some(&candidate) if candidate == *ch => vec![index + 1],
            _ => Vec::new(),
        },
        Ast::Any => match ctx.text.get(index) {
            Some(_) => vec![index + 1],
            None => Vec::new(),
        },
        Ast::Class(class) => match ctx.text.get(index) {
            Some(&candidate) if class.contains(candidate) => vec![index + 1],
            _ => Vec::new(),
        },
        Ast::AnchorStart => {
            if match_start == 0 {
                vec![index]
            } else {
                Vec::new()
            }
        }
        Ast::AnchorEnd => {
            if index == ctx.text_len {
                vec![index]
            } else {
                Vec::new()
            }
        }
        Ast::Sequence(nodes) => {
            let mut positions = vec![index];
            for node in nodes {
                let mut next = BTreeSet::new();
                for pos in &positions {
                    for matched in match_node(node, ctx, *pos, match_start) {
                        next.insert(matched);
                    }
                }
                if next.is_empty() {
                    return Vec::new();
                }
                positions = next.into_iter().collect();
            }
            positions
        }
        Ast::Alternate(nodes) => {
            let mut next = BTreeSet::new();
            for node in nodes {
                for matched in match_node(node, ctx, index, match_start) {
                    next.insert(matched);
                }
            }
            next.into_iter().collect()
        }
        Ast::Repeat { node, min, max } => match_repeat(node, *min, *max, ctx, index, match_start),
    }
}

fn match_repeat(
    node: &Ast,
    min: usize,
    max: Option<usize>,
    ctx: &MatchCtx<'_>,
    index: usize,
    match_start: usize,
) -> Vec<usize> {
    let mut results = BTreeSet::new();
    let mut stack = vec![(index, min, max)];
    while let Some((pos, min_left, max_left)) = stack.pop() {
        if min_left == 0 {
            results.insert(pos);
            if matches!(max_left, Some(0)) {
                continue;
            }
        }
        if let Some(0) = max_left {
            continue;
        }
        let next_min = min_left.saturating_sub(1);
        let next_max = max_left.map(|value| value - 1);
        for next_pos in match_node(node, ctx, pos, match_start) {
            if next_pos == pos {
                // The inner node matched an empty string.  Additional
                // iterations would loop forever, so terminate after ensuring
                // the minimum requirement is satisfied.
                if min_left == 0 {
                    results.insert(pos);
                }
                continue;
            }
            stack.push((next_pos, next_min, next_max));
        }
    }
    results.into_iter().collect()
}

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

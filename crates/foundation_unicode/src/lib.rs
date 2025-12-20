#![allow(clippy::needless_lifetimes)]
#![forbid(unsafe_code)]

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalizationAccuracy {
    Exact,
    Approximate,
}

impl NormalizationAccuracy {
    pub fn as_str(&self) -> &'static str {
        match self {
            NormalizationAccuracy::Exact => "exact",
            NormalizationAccuracy::Approximate => "approximate",
        }
    }

    pub fn is_exact(&self) -> bool {
        matches!(self, NormalizationAccuracy::Exact)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Normalized<'a> {
    text: Cow<'a, str>,
    accuracy: NormalizationAccuracy,
}

impl<'a> Normalized<'a> {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_owned(self) -> String {
        self.text.into_owned()
    }

    pub fn accuracy(&self) -> NormalizationAccuracy {
        self.accuracy
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Normalizer;

impl Normalizer {
    pub fn nfkc<'a>(&self, input: &'a str) -> Normalized<'a> {
        if input.is_empty() {
            return Normalized {
                text: Cow::Borrowed(""),
                accuracy: NormalizationAccuracy::Exact,
            };
        }
        if input.is_ascii() {
            return Normalized {
                text: Cow::Borrowed(input),
                accuracy: NormalizationAccuracy::Exact,
            };
        }
        normalize_fallback(input)
    }
}

fn normalize_fallback(input: &str) -> Normalized<'_> {
    use NormalizationAccuracy::{Approximate, Exact};

    let mut out = String::with_capacity(input.len());
    let mut accuracy = Exact;
    for ch in input.chars() {
        if ch.is_ascii() {
            out.push(ch);
            continue;
        }
        if let Some(acc) = compatibility_map(ch, &mut out) {
            if matches!(acc, Approximate) {
                accuracy = Approximate;
            }
            continue;
        }
        accuracy = Approximate;
        out.push(ch);
    }
    Normalized {
        text: Cow::Owned(out),
        accuracy,
    }
}

fn compatibility_map(ch: char, out: &mut String) -> Option<NormalizationAccuracy> {
    use NormalizationAccuracy::{Approximate, Exact};

    match ch {
        '\u{00A0}' | '\u{2000}'..='\u{200B}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => {
            out.push(' ');
            Some(Exact)
        }
        '\u{00B5}' => {
            out.push('μ');
            Some(Approximate)
        }
        '\u{00B7}' => {
            out.push('·');
            Some(Exact)
        }
        '\u{FF01}'..='\u{FF5E}' => {
            let converted = (ch as u32) - 0xFEE0;
            char::from_u32(converted).map(|ascii| {
                out.push(ascii);
                Exact
            })
        }
        '\u{2160}'..='\u{216F}' => {
            roman_upper(ch, out);
            Some(Exact)
        }
        '\u{2170}'..='\u{217F}' => {
            roman_lower(ch, out);
            Some(Exact)
        }
        _ => transliterate_latin1(ch, out).or_else(|| transliterate_greek(ch, out)),
    }
}

fn transliterate_latin1(ch: char, out: &mut String) -> Option<NormalizationAccuracy> {
    use NormalizationAccuracy::Approximate;

    let replacement = match ch {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => Some("A"),
        'Æ' => Some("AE"),
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => Some("C"),
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => Some("E"),
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' => Some("I"),
        'Ð' => Some("D"),
        'Ñ' => Some("N"),
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ő' | 'Ŏ' => Some("O"),
        'Œ' => Some("OE"),
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => Some("U"),
        'Ý' | 'Ÿ' | 'Ŷ' => Some("Y"),
        'Þ' => Some("Th"),
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => Some("a"),
        'æ' => Some("ae"),
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => Some("c"),
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => Some("e"),
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => Some("i"),
        'ð' => Some("d"),
        'ñ' => Some("n"),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' | 'ŏ' => Some("o"),
        'œ' => Some("oe"),
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => Some("u"),
        'ý' | 'ÿ' | 'ŷ' => Some("y"),
        'þ' => Some("th"),
        'ß' => Some("ss"),
        'Ł' => Some("L"),
        'ł' => Some("l"),
        'Ś' | 'Š' | 'Ŝ' => Some("S"),
        'ś' | 'š' | 'ŝ' => Some("s"),
        'Ź' | 'Ż' | 'Ž' => Some("Z"),
        'ź' | 'ż' | 'ž' => Some("z"),
        'Ŕ' | 'Ŗ' => Some("R"),
        'ŕ' | 'ŗ' => Some("r"),
        'Ĺ' | 'Ļ' | 'Ľ' => Some("L"),
        'ĺ' | 'ļ' | 'ľ' => Some("l"),
        'Ť' | 'Ţ' => Some("T"),
        'ť' | 'ţ' => Some("t"),
        'Ģ' => Some("G"),
        'ģ' => Some("g"),
        'Ķ' => Some("K"),
        'ķ' => Some("k"),
        'Ħ' => Some("H"),
        'ħ' => Some("h"),
        'Ŋ' => Some("N"),
        'ŋ' => Some("n"),
        'ƒ' => Some("f"),
        'Ĳ' => Some("IJ"),
        'ĳ' => Some("ij"),
        'ſ' => Some("s"),
        _ => None,
    }?;

    out.push_str(replacement);
    Some(Approximate)
}

fn transliterate_greek(ch: char, out: &mut String) -> Option<NormalizationAccuracy> {
    use NormalizationAccuracy::Approximate;

    let replacement = match ch {
        'Α' | 'Ά' => Some("A"),
        'Β' => Some("B"),
        'Γ' => Some("G"),
        'Δ' => Some("D"),
        'Ε' | 'Έ' => Some("E"),
        'Ζ' => Some("Z"),
        'Η' | 'Ή' => Some("E"),
        'Θ' => Some("Th"),
        'Ι' | 'Ί' | 'Ϊ' => Some("I"),
        'Κ' => Some("K"),
        'Λ' => Some("L"),
        'Μ' => Some("M"),
        'Ν' => Some("N"),
        'Ξ' => Some("X"),
        'Ο' | 'Ό' => Some("O"),
        'Π' => Some("P"),
        'Ρ' => Some("R"),
        'Σ' => Some("S"),
        'Τ' => Some("T"),
        'Υ' | 'Ύ' | 'Ϋ' => Some("Y"),
        'Φ' => Some("F"),
        'Χ' => Some("Ch"),
        'Ψ' => Some("Ps"),
        'Ω' | 'Ώ' => Some("O"),
        'α' | 'ά' => Some("a"),
        'β' => Some("b"),
        'γ' => Some("g"),
        'δ' => Some("d"),
        'ε' | 'έ' => Some("e"),
        'ζ' => Some("z"),
        'η' | 'ή' => Some("e"),
        'θ' => Some("th"),
        'ι' | 'ί' | 'ϊ' | 'ΐ' => Some("i"),
        'κ' => Some("k"),
        'λ' => Some("l"),
        'μ' => Some("m"),
        'ν' => Some("n"),
        'ξ' => Some("x"),
        'ο' | 'ό' => Some("o"),
        'π' => Some("p"),
        'ρ' => Some("r"),
        'σ' | 'ς' => Some("s"),
        'τ' => Some("t"),
        'υ' | 'ύ' | 'ϋ' | 'ΰ' => Some("y"),
        'φ' => Some("f"),
        'χ' => Some("ch"),
        'ψ' => Some("ps"),
        'ω' | 'ώ' => Some("o"),
        _ => None,
    }?;

    out.push_str(replacement);
    Some(Approximate)
}

fn roman_upper(ch: char, out: &mut String) {
    const MAP: [&str; 16] = [
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV",
        "XV", "XVI",
    ];
    let idx = (ch as u32) - 0x2160;
    if let Some(seq) = MAP.get(idx as usize) {
        out.push_str(seq);
    }
}

fn roman_lower(ch: char, out: &mut String) {
    const MAP: [&str; 16] = [
        "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv",
        "xv", "xvi",
    ];
    let idx = (ch as u32) - 0x2170;
    if let Some(seq) = MAP.get(idx as usize) {
        out.push_str(seq);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passthrough_is_exact() {
        let normalizer = Normalizer::default();
        let normalized = normalizer.nfkc("hello");
        assert_eq!(normalized.as_str(), "hello");
        assert_eq!(normalized.accuracy(), NormalizationAccuracy::Exact);
    }

    #[test]
    fn converts_full_width_ascii() {
        let normalizer = Normalizer::default();
        let normalized = normalizer.nfkc("Ｈｅｌｌｏ　Ｗｏｒｌｄ");
        assert_eq!(normalized.as_str(), "Hello World");
        assert_eq!(normalized.accuracy(), NormalizationAccuracy::Exact);
    }

    #[test]
    fn compatibility_decomposition_marks_approximate() {
        let normalizer = Normalizer::default();
        let normalized = normalizer.nfkc("école");
        assert_eq!(normalized.as_str(), "ecole");
        assert_eq!(normalized.accuracy(), NormalizationAccuracy::Approximate);
    }

    #[test]
    fn transliterates_latin1_accents() {
        let normalizer = Normalizer::default();
        let normalized = normalizer.nfkc("façade");
        assert_eq!(normalized.as_str(), "facade");
        assert_eq!(normalized.accuracy(), NormalizationAccuracy::Approximate);
    }

    #[test]
    fn transliterates_greek_letters() {
        let normalizer = Normalizer::default();
        let normalized = normalizer.nfkc("Ωμέγα");
        assert_eq!(normalized.as_str(), "Omega");
        assert_eq!(normalized.accuracy(), NormalizationAccuracy::Approximate);
    }
}

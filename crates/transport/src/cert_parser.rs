use foundation_time::UtcDateTime;
use std::fmt;

#[derive(Debug)]
pub struct CertificateMetadata<'a> {
    pub algorithm_oid: String,
    pub public_key: &'a [u8],
    pub not_before: UtcDateTime,
    pub not_after: UtcDateTime,
}

#[derive(Debug)]
pub enum ParseError {
    UnexpectedEof,
    InvalidLength,
    InvalidTag(&'static str),
    InvalidOid,
    InvalidTime,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseError::InvalidLength => write!(f, "invalid length"),
            ParseError::InvalidTag(tag) => write!(f, "invalid tag for {tag}"),
            ParseError::InvalidOid => write!(f, "invalid object identifier"),
            ParseError::InvalidTime => write!(f, "invalid time encoding"),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse_certificate_metadata<'a>(
    input: &'a [u8],
) -> Result<CertificateMetadata<'a>, ParseError> {
    let mut cert_reader = DerReader::new(input);
    let mut certificate = cert_reader.read_sequence()?;
    let mut tbs = certificate.read_sequence()?;
    certificate.skip()?; // signatureAlgorithm
    certificate.skip()?; // signatureValue

    if matches!(tbs.peek_tag(), Some(0xa0)) {
        let mut version = tbs.read_explicit(0xa0)?;
        version.skip()?; // version value
        if !version.is_empty() {
            return Err(ParseError::InvalidLength);
        }
    }

    tbs.skip()?; // serialNumber
    tbs.skip()?; // signature
    tbs.skip()?; // issuer

    let mut validity = tbs.read_sequence()?;
    let not_before = parse_time(validity.read_value()?)?;
    let not_after = parse_time(validity.read_value()?)?;

    tbs.skip()?; // subject

    let mut spki = tbs.read_sequence()?;
    let mut algorithm = spki.read_sequence()?;
    let algorithm_oid_bytes = algorithm.read_oid()?;
    while !algorithm.is_empty() {
        algorithm.skip()?;
    }
    let public_key = spki.read_bit_string()?;

    Ok(CertificateMetadata {
        algorithm_oid: oid_to_string(algorithm_oid_bytes)?,
        public_key,
        not_before,
        not_after,
    })
}

struct DerReader<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> DerReader<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    fn is_empty(&self) -> bool {
        self.position >= self.input.len()
    }

    fn peek_tag(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn read_value(&mut self) -> Result<DerValue<'a>, ParseError> {
        let tag = self.read_byte()?;
        let length = self.read_length()?;
        if self.remaining() < length {
            return Err(ParseError::UnexpectedEof);
        }
        let start = self.position;
        self.position += length;
        Ok(DerValue {
            tag,
            data: &self.input[start..start + length],
        })
    }

    fn read_sequence(&mut self) -> Result<DerReader<'a>, ParseError> {
        let value = self.read_value()?;
        if value.tag != 0x30 {
            return Err(ParseError::InvalidTag("SEQUENCE"));
        }
        Ok(DerReader::new(value.data))
    }

    fn read_explicit(&mut self, tag: u8) -> Result<DerReader<'a>, ParseError> {
        let value = self.read_value()?;
        if value.tag != tag {
            return Err(ParseError::InvalidTag("EXPLICIT"));
        }
        Ok(DerReader::new(value.data))
    }

    fn read_oid(&mut self) -> Result<&'a [u8], ParseError> {
        let value = self.read_value()?;
        if value.tag != 0x06 {
            return Err(ParseError::InvalidTag("OBJECT IDENTIFIER"));
        }
        Ok(value.data)
    }

    fn read_bit_string(&mut self) -> Result<&'a [u8], ParseError> {
        let value = self.read_value()?;
        if value.tag != 0x03 {
            return Err(ParseError::InvalidTag("BIT STRING"));
        }
        if value.data.is_empty() {
            return Err(ParseError::InvalidLength);
        }
        if value.data[0] != 0 {
            return Err(ParseError::InvalidLength);
        }
        Ok(&value.data[1..])
    }

    fn skip(&mut self) -> Result<(), ParseError> {
        let _ = self.read_value()?;
        Ok(())
    }

    fn read_byte(&mut self) -> Result<u8, ParseError> {
        if self.position >= self.input.len() {
            return Err(ParseError::UnexpectedEof);
        }
        let byte = self.input[self.position];
        self.position += 1;
        Ok(byte)
    }

    fn read_length(&mut self) -> Result<usize, ParseError> {
        let first = self.read_byte()?;
        if first & 0x80 == 0 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(ParseError::InvalidLength);
        }
        if self.remaining() < count {
            return Err(ParseError::UnexpectedEof);
        }
        let mut length = 0usize;
        for _ in 0..count {
            length = (length << 8) | (self.read_byte()? as usize);
        }
        Ok(length)
    }

    fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.position)
    }
}

struct DerValue<'a> {
    tag: u8,
    data: &'a [u8],
}

fn parse_time(value: DerValue<'_>) -> Result<UtcDateTime, ParseError> {
    match value.tag {
        0x17 => parse_utc_time(value.data),
        0x18 => parse_generalized_time(value.data),
        _ => Err(ParseError::InvalidTag("Time")),
    }
}

fn parse_utc_time(data: &[u8]) -> Result<UtcDateTime, ParseError> {
    if data.len() < 11 || *data.last().unwrap() != b'Z' {
        return Err(ParseError::InvalidTime);
    }
    let digits = &data[..data.len() - 1];
    let (year_low, rest) = digits.split_at(2);
    let mut year = parse_digits(year_low)? as i32;
    year += if year >= 50 { 1900 } else { 2000 };
    parse_time_components(year, rest)
}

fn parse_generalized_time(data: &[u8]) -> Result<UtcDateTime, ParseError> {
    if data.len() < 13 || *data.last().unwrap() != b'Z' {
        return Err(ParseError::InvalidTime);
    }
    let digits = &data[..data.len() - 1];
    let (year_bytes, rest) = digits.split_at(4);
    let year = parse_digits(year_bytes)? as i32;
    parse_time_components(year, rest)
}

fn parse_time_components(year: i32, digits: &[u8]) -> Result<UtcDateTime, ParseError> {
    if digits.len() < 8 {
        return Err(ParseError::InvalidTime);
    }
    let month = parse_digits(&digits[0..2])?;
    let day = parse_digits(&digits[2..4])?;
    let hour = parse_digits(&digits[4..6])?;
    let minute = parse_digits(&digits[6..8])?;
    let second = if digits.len() >= 10 {
        parse_digits(&digits[8..10])?
    } else {
        0
    };
    utc_from_components(year, month, day, hour, minute, second)
}

fn parse_digits(bytes: &[u8]) -> Result<u32, ParseError> {
    if bytes.is_empty() {
        return Err(ParseError::InvalidTime);
    }
    let mut value = 0u32;
    for &b in bytes {
        if !(b'0'..=b'9').contains(&b) {
            return Err(ParseError::InvalidTime);
        }
        value = value * 10 + (b - b'0') as u32;
    }
    Ok(value)
}

fn utc_from_components(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Result<UtcDateTime, ParseError> {
    if month == 0 || month > 12 {
        return Err(ParseError::InvalidTime);
    }
    if day == 0 || day > days_in_month(year, month) {
        return Err(ParseError::InvalidTime);
    }
    if hour > 23 || minute > 59 || second > 60 {
        return Err(ParseError::InvalidTime);
    }
    let days = days_from_civil(year, month, day);
    let seconds = days
        .checked_mul(86_400)
        .and_then(|base| base.checked_add((hour * 3_600 + minute * 60 + second) as i64))
        .ok_or(ParseError::InvalidTime)?;
    UtcDateTime::from_unix_timestamp(seconds).map_err(|_| ParseError::InvalidTime)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let mut y = year as i64;
    let m = month as i64;
    let d = day as i64;
    y -= (m <= 2) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn oid_to_string(data: &[u8]) -> Result<String, ParseError> {
    if data.is_empty() {
        return Err(ParseError::InvalidOid);
    }
    let first = data[0];
    let first_component = (first / 40) as u32;
    let second_component = (first % 40) as u32;
    let mut components = vec![first_component, second_component];
    let mut value: u64 = 0;
    for &byte in &data[1..] {
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            components.push(value as u32);
            value = 0;
        }
    }
    if value != 0 {
        return Err(ParseError::InvalidOid);
    }
    let mut out = String::new();
    for (idx, component) in components.iter().enumerate() {
        if idx > 0 {
            out.push('.');
        }
        out.push_str(&component.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_tls::{generate_self_signed_ed25519, SelfSignedCertParams};

    #[test]
    fn parses_basic_certificate_metadata() {
        let params = SelfSignedCertParams::builder()
            .subject_cn("test")
            .validity(
                UtcDateTime::from_unix_timestamp(1_600_000_000).unwrap(),
                UtcDateTime::from_unix_timestamp(1_600_086_400).unwrap(),
            )
            .serial([0u8; 16])
            .build()
            .unwrap();
        let cert_result = generate_self_signed_ed25519(&params).unwrap();
        let metadata = parse_certificate_metadata(&cert_result.certificate).unwrap();
        assert_eq!(metadata.algorithm_oid, "1.3.101.112");
        assert_eq!(metadata.public_key.len(), 32);
        assert!(metadata.not_before < metadata.not_after);
    }

    #[test]
    fn days_from_civil_matches_known_values() {
        let unix_epoch = days_from_civil(1970, 1, 1);
        assert_eq!(unix_epoch, 0);
        let next_day = days_from_civil(1970, 1, 2);
        assert_eq!(next_day, 1);
        let leap = days_from_civil(2000, 2, 29);
        let prev = days_from_civil(2000, 2, 28);
        assert_eq!(leap - prev, 1);
    }
}

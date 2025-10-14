#![allow(clippy::too_many_lines)]

use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;

use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::net::peer::{DropReason, HandshakeError, PeerMetrics, PeerReputation};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};

/// Result alias for encoding routines.
pub type EncodeResult<T> = Result<T, EncodeError>;

/// Error raised by the manual peer metrics encoder.
#[derive(Debug)]
pub enum EncodeError {
    /// Collection length exceeded `u64::MAX`.
    LengthOverflow(&'static str),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::LengthOverflow(name) => {
                write!(f, "{name} entry count exceeds u64::MAX")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Encode [`PeerMetrics`] into the legacy binary representation.
pub fn encode(metrics: &PeerMetrics) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    writer.write_u64(16);

    writer.write_string("requests");
    writer.write_u64(metrics.requests);

    writer.write_string("bytes_sent");
    writer.write_u64(metrics.bytes_sent);

    writer.write_string("sends");
    writer.write_u64(metrics.sends);

    writer.write_string("drops");
    write_drop_map(&mut writer, &metrics.drops)?;

    writer.write_string("handshake_fail");
    write_handshake_map(&mut writer, &metrics.handshake_fail)?;

    writer.write_string("handshake_success");
    writer.write_u64(metrics.handshake_success);

    writer.write_string("last_handshake_ms");
    writer.write_u64(metrics.last_handshake_ms);

    writer.write_string("tls_errors");
    writer.write_u64(metrics.tls_errors);

    writer.write_string("reputation");
    write_reputation(&mut writer, &metrics.reputation);

    writer.write_string("last_updated");
    writer.write_u64(metrics.last_updated);

    writer.write_string("req_avg");
    writer.write_f64(metrics.req_avg);

    writer.write_string("byte_avg");
    writer.write_f64(metrics.byte_avg);

    writer.write_string("throttled_until");
    writer.write_u64(metrics.throttled_until);

    writer.write_string("throttle_reason");
    writer.write_option_with(metrics.throttle_reason.as_deref(), |writer, value| {
        writer.write_string(value)
    });

    writer.write_string("backoff_level");
    writer.write_u32(metrics.backoff_level);

    writer.write_string("sec_start");
    writer.write_u64(metrics.sec_start);

    Ok(writer.finish())
}

/// Decode [`PeerMetrics`] from the legacy binary representation.
pub fn decode(bytes: &[u8]) -> binary_struct::Result<PeerMetrics> {
    let mut reader = Reader::new(bytes);

    let mut requests = None;
    let mut bytes_sent = None;
    let mut sends = None;
    let mut drops = None;
    let mut handshake_fail = None;
    let mut handshake_success = None;
    let mut last_handshake_ms = None;
    let mut tls_errors = None;
    let mut reputation = None;
    let mut last_updated = None;
    let mut req_avg = None;
    let mut byte_avg = None;
    let mut throttled_until = None;
    let mut throttle_reason = None;
    let mut backoff_level = None;
    let mut sec_start = None;

    decode_struct(&mut reader, None, |key, reader| match key {
        "requests" => assign_once(&mut requests, reader.read_u64()?, "requests"),
        "bytes_sent" => assign_once(&mut bytes_sent, reader.read_u64()?, "bytes_sent"),
        "sends" => assign_once(&mut sends, reader.read_u64()?, "sends"),
        "drops" => assign_once(&mut drops, read_drop_map(reader)?, "drops"),
        "handshake_fail" => assign_once(
            &mut handshake_fail,
            read_handshake_map(reader)?,
            "handshake_fail",
        ),
        "handshake_success" => assign_once(
            &mut handshake_success,
            reader.read_u64()?,
            "handshake_success",
        ),
        "last_handshake_ms" => assign_once(
            &mut last_handshake_ms,
            reader.read_u64()?,
            "last_handshake_ms",
        ),
        "tls_errors" => assign_once(&mut tls_errors, reader.read_u64()?, "tls_errors"),
        "reputation" => assign_once(&mut reputation, read_reputation(reader)?, "reputation"),
        "last_updated" => assign_once(&mut last_updated, reader.read_u64()?, "last_updated"),
        "req_avg" => assign_once(&mut req_avg, reader.read_f64()?, "req_avg"),
        "byte_avg" => assign_once(&mut byte_avg, reader.read_f64()?, "byte_avg"),
        "throttled_until" => {
            assign_once(&mut throttled_until, reader.read_u64()?, "throttled_until")
        }
        "throttle_reason" => assign_once(
            &mut throttle_reason,
            reader.read_option_with(|reader| reader.read_string())?,
            "throttle_reason",
        ),
        "backoff_level" => assign_once(&mut backoff_level, reader.read_u32()?, "backoff_level"),
        "sec_start" => assign_once(&mut sec_start, reader.read_u64()?, "sec_start"),
        other => Err(DecodeError::UnknownField(other.to_string())),
    })?;

    ensure_exhausted(&reader)?;

    let mut metrics = PeerMetrics::default();
    if let Some(value) = requests {
        metrics.requests = value;
    }
    if let Some(value) = bytes_sent {
        metrics.bytes_sent = value;
    }
    if let Some(value) = sends {
        metrics.sends = value;
    }
    if let Some(value) = drops {
        metrics.drops = value;
    }
    if let Some(value) = handshake_fail {
        metrics.handshake_fail = value;
    }
    if let Some(value) = handshake_success {
        metrics.handshake_success = value;
    }
    if let Some(value) = last_handshake_ms {
        metrics.last_handshake_ms = value;
    }
    if let Some(value) = tls_errors {
        metrics.tls_errors = value;
    }
    if let Some(value) = reputation {
        metrics.reputation = value;
    }
    if let Some(value) = last_updated {
        metrics.last_updated = value;
    }
    if let Some(value) = req_avg {
        metrics.req_avg = value;
    }
    if let Some(value) = byte_avg {
        metrics.byte_avg = value;
    }
    if let Some(value) = throttled_until {
        metrics.throttled_until = value;
    }
    if let Some(value) = throttle_reason {
        metrics.throttle_reason = value;
    }
    if let Some(value) = backoff_level {
        metrics.backoff_level = value;
    }
    if let Some(value) = sec_start {
        metrics.sec_start = value;
    }

    Ok(metrics)
}

fn write_drop_map(writer: &mut Writer, map: &HashMap<DropReason, u64>) -> EncodeResult<()> {
    let mut entries: Vec<(DropReason, u64)> = map.iter().map(|(k, v)| (*k, *v)).collect();
    entries.sort_by_key(|(reason, _)| drop_reason_to_index(*reason));
    writer.write_u64(to_u64(entries.len(), "drops")?);
    for (reason, value) in entries {
        writer.write_u32(drop_reason_to_index(reason));
        writer.write_u64(value);
    }
    Ok(())
}

fn write_handshake_map(
    writer: &mut Writer,
    map: &HashMap<HandshakeError, u64>,
) -> EncodeResult<()> {
    let mut entries: Vec<(HandshakeError, u64)> = map.iter().map(|(k, v)| (*k, *v)).collect();
    entries.sort_by_key(|(err, _)| handshake_error_to_index(*err));
    writer.write_u64(to_u64(entries.len(), "handshake_fail")?);
    for (error, value) in entries {
        writer.write_u32(handshake_error_to_index(error));
        writer.write_u64(value);
    }
    Ok(())
}

fn write_reputation(writer: &mut Writer, reputation: &PeerReputation) {
    writer.write_struct(|s| {
        s.field_f64("score", reputation.score);
    });
}

fn read_drop_map(reader: &mut Reader<'_>) -> Result<HashMap<DropReason, u64>, DecodeError> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut map = HashMap::with_capacity(len);
    for _ in 0..len {
        let idx = reader.read_u32()?;
        let reason = drop_reason_from_index(idx)?;
        let value = reader.read_u64()?;
        map.insert(reason, value);
    }
    Ok(map)
}

fn read_handshake_map(
    reader: &mut Reader<'_>,
) -> Result<HashMap<HandshakeError, u64>, DecodeError> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut map = HashMap::with_capacity(len);
    for _ in 0..len {
        let idx = reader.read_u32()?;
        let error = handshake_error_from_index(idx)?;
        let value = reader.read_u64()?;
        map.insert(error, value);
    }
    Ok(map)
}

fn read_reputation(reader: &mut Reader<'_>) -> Result<PeerReputation, DecodeError> {
    let mut score = None;
    decode_struct(reader, Some(1), |key, reader| match key {
        "score" => assign_once(&mut score, reader.read_f64()?, "score"),
        other => Err(DecodeError::UnknownField(other.to_string())),
    })?;
    let mut reputation = PeerReputation::default();
    if let Some(value) = score {
        reputation.score = value;
    }
    Ok(reputation)
}

fn drop_reason_to_index(reason: DropReason) -> u32 {
    match reason {
        DropReason::RateLimit => 0,
        DropReason::Malformed => 1,
        DropReason::Blacklist => 2,
        DropReason::Duplicate => 3,
        DropReason::TooBusy => 4,
        DropReason::Other => 5,
    }
}

fn drop_reason_from_index(value: u32) -> Result<DropReason, DecodeError> {
    match value {
        0 => Ok(DropReason::RateLimit),
        1 => Ok(DropReason::Malformed),
        2 => Ok(DropReason::Blacklist),
        3 => Ok(DropReason::Duplicate),
        4 => Ok(DropReason::TooBusy),
        5 => Ok(DropReason::Other),
        value => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "DropReason",
            value,
        }),
    }
}

fn handshake_error_to_index(error: HandshakeError) -> u32 {
    match error {
        HandshakeError::Tls => 0,
        HandshakeError::Version => 1,
        HandshakeError::Timeout => 2,
        HandshakeError::Certificate => 3,
        HandshakeError::Other => 4,
    }
}

fn handshake_error_from_index(value: u32) -> Result<HandshakeError, DecodeError> {
    match value {
        0 => Ok(HandshakeError::Tls),
        1 => Ok(HandshakeError::Version),
        2 => Ok(HandshakeError::Timeout),
        3 => Ok(HandshakeError::Certificate),
        4 => Ok(HandshakeError::Other),
        value => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "HandshakeError",
            value,
        }),
    }
}

fn to_u64(len: usize, name: &'static str) -> EncodeResult<u64> {
    u64::try_from(len).map_err(|_| EncodeError::LengthOverflow(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::peer::{DropReason, HandshakeError, PeerMetrics};
    use crate::util::binary_codec;

    fn sample_metrics() -> PeerMetrics {
        let mut metrics = PeerMetrics::default();
        metrics.requests = 42;
        metrics.bytes_sent = 512;
        metrics.sends = 7;
        metrics
            .drops
            .extend([(DropReason::Malformed, 3), (DropReason::TooBusy, 1)]);
        metrics.handshake_fail.insert(HandshakeError::Tls, 5);
        metrics.handshake_fail.insert(HandshakeError::Timeout, 2);
        metrics.handshake_success = 11;
        metrics.last_handshake_ms = 1234;
        metrics.tls_errors = 9;
        metrics.reputation.score = 0.75;
        metrics.last_updated = 777;
        metrics.req_avg = 1.25;
        metrics.byte_avg = 2048.0;
        metrics.throttled_until = 888;
        metrics.throttle_reason = Some("bandwidth".to_string());
        metrics.backoff_level = 3;
        metrics.sec_start = 900;
        metrics.sec_requests = 55;
        metrics.sec_bytes = 99;
        metrics.breach_count = 4;
        metrics
    }

    #[test]
    fn encoding_matches_legacy_codec() {
        let metrics = sample_metrics();
        let legacy = binary_codec::serialize(&metrics).expect("legacy encode");
        let manual = encode(&metrics).expect("manual encode");
        assert_eq!(legacy, manual);
    }

    #[test]
    fn decode_round_trips_manual_encoding() {
        let metrics = sample_metrics();
        let manual = encode(&metrics).expect("manual encode");
        let decoded = decode(&manual).expect("manual decode");

        assert_eq!(decoded.requests, metrics.requests);
        assert_eq!(decoded.bytes_sent, metrics.bytes_sent);
        assert_eq!(decoded.sends, metrics.sends);
        assert_eq!(decoded.drops, metrics.drops);
        assert_eq!(decoded.handshake_fail, metrics.handshake_fail);
        assert_eq!(decoded.handshake_success, metrics.handshake_success);
        assert_eq!(decoded.last_handshake_ms, metrics.last_handshake_ms);
        assert_eq!(decoded.tls_errors, metrics.tls_errors);
        assert!((decoded.reputation.score - metrics.reputation.score).abs() < f64::EPSILON);
        assert_eq!(decoded.last_updated, metrics.last_updated);
        assert!((decoded.req_avg - metrics.req_avg).abs() < f64::EPSILON);
        assert!((decoded.byte_avg - metrics.byte_avg).abs() < f64::EPSILON);
        assert_eq!(decoded.throttled_until, metrics.throttled_until);
        assert_eq!(decoded.throttle_reason, metrics.throttle_reason);
        assert_eq!(decoded.backoff_level, metrics.backoff_level);
        assert_eq!(decoded.sec_start, metrics.sec_start);
        assert_eq!(decoded.sec_requests, 0);
        assert_eq!(decoded.sec_bytes, 0);
        assert_eq!(decoded.breach_count, 0);
    }

    #[test]
    fn decode_legacy_bytes_matches_struct() {
        let metrics = sample_metrics();
        let legacy = binary_codec::serialize(&metrics).expect("legacy encode");
        let decoded = decode(&legacy).expect("legacy decode");

        assert_eq!(decoded.requests, metrics.requests);
        assert_eq!(decoded.bytes_sent, metrics.bytes_sent);
        assert_eq!(decoded.sends, metrics.sends);
        assert_eq!(decoded.drops, metrics.drops);
        assert_eq!(decoded.handshake_fail, metrics.handshake_fail);
        assert_eq!(decoded.handshake_success, metrics.handshake_success);
        assert_eq!(decoded.last_handshake_ms, metrics.last_handshake_ms);
        assert_eq!(decoded.tls_errors, metrics.tls_errors);
        assert!((decoded.reputation.score - metrics.reputation.score).abs() < f64::EPSILON);
        assert_eq!(decoded.last_updated, metrics.last_updated);
        assert!((decoded.req_avg - metrics.req_avg).abs() < f64::EPSILON);
        assert!((decoded.byte_avg - metrics.byte_avg).abs() < f64::EPSILON);
        assert_eq!(decoded.throttled_until, metrics.throttled_until);
        assert_eq!(decoded.throttle_reason, metrics.throttle_reason);
        assert_eq!(decoded.backoff_level, metrics.backoff_level);
        assert_eq!(decoded.sec_start, metrics.sec_start);
    }

    #[test]
    fn invalid_enum_discriminant_errors() {
        let mut writer = Writer::new();
        writer.write_u64(4);
        writer.write_string("drops");
        writer.write_u64(1);
        writer.write_u32(99);
        writer.write_u64(1);
        writer.write_string("handshake_fail");
        writer.write_u64(0);
        writer.write_string("requests");
        writer.write_u64(0);
        writer.write_string("bytes_sent");
        writer.write_u64(0);
        let bytes = writer.finish();
        let err = match decode(&bytes) {
            Ok(_) => panic!("expected invalid enum error"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            DecodeError::InvalidEnumDiscriminant { ty, value }
                if ty == "DropReason" && value == 99
        ));
    }
}

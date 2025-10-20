use foundation_serialization::binary_cursor::{Reader, Writer};

use super::handle_registry::HandleRecord;
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};

#[cfg(feature = "pq-crypto")]
const FIELD_COUNT: u64 = 6;
#[cfg(not(feature = "pq-crypto"))]
const FIELD_COUNT: u64 = 5;

/// Encode a [`HandleRecord`] into the legacy canonical binary layout.
pub fn encode_record(record: &HandleRecord) -> Vec<u8> {
    let mut writer = Writer::new();
    write_record(&mut writer, record);
    writer.finish()
}

/// Decode a [`HandleRecord`] produced by the canonical binary layout.
pub fn decode_record(bytes: &[u8]) -> binary_struct::Result<HandleRecord> {
    let mut reader = Reader::new(bytes);
    let record = read_record(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(record)
}

/// Encode a string using the canonical binary layout.
pub fn encode_string(value: &str) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_string(value);
    writer.finish()
}

/// Decode a string that was encoded via [`encode_string`].
pub fn decode_string(bytes: &[u8]) -> binary_struct::Result<String> {
    let mut reader = Reader::new(bytes);
    let value = reader.read_string()?;
    ensure_exhausted(&reader)?;
    Ok(value)
}

/// Encode a `u64` using the canonical binary layout.
pub fn encode_u64(value: u64) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_u64(value);
    writer.finish()
}

/// Decode a `u64` that was encoded via [`encode_u64`].
pub fn decode_u64(bytes: &[u8]) -> binary_struct::Result<u64> {
    let mut reader = Reader::new(bytes);
    let value = reader.read_u64()?;
    ensure_exhausted(&reader)?;
    Ok(value)
}

fn write_record(writer: &mut Writer, record: &HandleRecord) {
    writer.write_struct(|s| {
        s.field_string("address", &record.address);
        s.field_u64("created_at", record.created_at);
        s.field_with("attest_sig", |w| w.write_bytes(&record.attest_sig));
        s.field_u64("nonce", record.nonce);
        s.field_with("version", |w| w.write_u16(record.version));
        #[cfg(feature = "pq-crypto")]
        {
            s.field_with("pq_pubkey", |w| {
                w.write_option_with(record.pq_pubkey.as_ref(), |writer, value| {
                    writer.write_bytes(value);
                });
            });
        }
    });
}

fn read_record(reader: &mut Reader<'_>) -> binary_struct::Result<HandleRecord> {
    let mut address = None;
    let mut created_at = None;
    let mut attest_sig = None;
    let mut nonce = None;
    let mut version = None;
    #[cfg(feature = "pq-crypto")]
    let mut pq_pubkey: Option<Option<Vec<u8>>> = None;

    decode_struct(reader, Some(FIELD_COUNT), |key, reader| match key {
        "address" => {
            let value = reader.read_string()?;
            assign_once(&mut address, value, "address")
        }
        "created_at" => {
            let value = reader.read_u64()?;
            assign_once(&mut created_at, value, "created_at")
        }
        "attest_sig" => {
            let value = reader.read_bytes()?;
            assign_once(&mut attest_sig, value, "attest_sig")
        }
        "nonce" => {
            let value = reader.read_u64()?;
            assign_once(&mut nonce, value, "nonce")
        }
        "version" => {
            let value = reader.read_u16()?;
            assign_once(&mut version, value, "version")
        }
        #[cfg(feature = "pq-crypto")]
        "pq_pubkey" => {
            let value = reader.read_option_with(|reader| reader.read_bytes())?;
            assign_once(&mut pq_pubkey, value, "pq_pubkey")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(HandleRecord {
        address: address.ok_or(DecodeError::MissingField("address"))?,
        created_at: created_at.ok_or(DecodeError::MissingField("created_at"))?,
        attest_sig: attest_sig.ok_or(DecodeError::MissingField("attest_sig"))?,
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        version: version.ok_or(DecodeError::MissingField("version"))?,
        #[cfg(feature = "pq-crypto")]
        pq_pubkey: pq_pubkey.unwrap_or(None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::handle_registry::HandleRecord;
    use rand::{rngs::StdRng, RngCore};

    fn sample_record() -> HandleRecord {
        HandleRecord {
            address: "addr".into(),
            created_at: 1_650_000_000,
            attest_sig: vec![1, 2, 3, 4],
            nonce: 7,
            version: 2,
            #[cfg(feature = "pq-crypto")]
            pq_pubkey: Some(vec![9, 9, 9]),
        }
    }

    #[test]
    fn record_round_trip() {
        let record = sample_record();
        let encoded = encode_record(&record);
        let decoded = decode_record(&encoded).expect("decode");
        assert_eq!(record.address, decoded.address);
        assert_eq!(record.created_at, decoded.created_at);
        assert_eq!(record.attest_sig, decoded.attest_sig);
        assert_eq!(record.nonce, decoded.nonce);
        assert_eq!(record.version, decoded.version);
        #[cfg(feature = "pq-crypto")]
        {
            assert_eq!(record.pq_pubkey, decoded.pq_pubkey);
        }
    }

    #[test]
    fn record_compatibility_with_legacy() {
        let record = sample_record();
        let encoded = encode_record(&record);
        let mut reader = Reader::new(&encoded);
        assert_eq!(reader.read_u64().expect("field count"), FIELD_COUNT);
        assert_eq!(reader.read_string().expect("address key"), "address");
        assert_eq!(reader.read_string().expect("address"), record.address);
        assert_eq!(reader.read_string().expect("created key"), "created_at");
        assert_eq!(reader.read_u64().expect("created_at"), record.created_at);
        assert_eq!(reader.read_string().expect("attest key"), "attest_sig");
        assert_eq!(
            reader.read_u64().expect("attest len"),
            record.attest_sig.len() as u64
        );
        for byte in &record.attest_sig {
            assert_eq!(reader.read_u8().expect("attest byte"), *byte);
        }
        assert_eq!(reader.read_string().expect("nonce key"), "nonce");
        assert_eq!(reader.read_u64().expect("nonce"), record.nonce);
        assert_eq!(reader.read_string().expect("version key"), "version");
        assert_eq!(reader.read_u16().expect("version"), record.version);
        #[cfg(feature = "pq-crypto")]
        {
            assert_eq!(reader.read_string().expect("pq key"), "pq_pubkey");
            if let Some(pk) = record.pq_pubkey.as_ref() {
                assert!(reader.read_bool().expect("pq present"));
                assert_eq!(reader.read_u64().expect("pq len"), pk.len() as u64);
                for byte in pk {
                    assert_eq!(reader.read_u8().expect("pq byte"), *byte);
                }
            } else {
                assert!(!reader.read_bool().expect("pq absent"));
            }
        }
    }

    #[test]
    fn string_round_trip() {
        let value = "example-handle";
        let encoded = encode_string(value);
        let decoded = decode_string(&encoded).expect("decode");
        assert_eq!(value, decoded);
    }

    #[test]
    fn string_compatibility_with_legacy() {
        let value = "owner";
        let encoded = encode_string(value);
        let mut reader = Reader::new(&encoded);
        assert_eq!(reader.read_u64().expect("len"), value.len() as u64);
        for byte in value.as_bytes() {
            assert_eq!(reader.read_u8().expect("byte"), *byte);
        }
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn u64_round_trip() {
        let value = 42u64;
        let encoded = encode_u64(value);
        let decoded = decode_u64(&encoded).expect("decode");
        assert_eq!(value, decoded);
    }

    #[test]
    fn u64_compatibility_with_legacy() {
        let value = 99u64;
        let encoded = encode_u64(value);
        let mut reader = Reader::new(&encoded);
        assert_eq!(reader.read_u64().expect("value"), value);
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn rejects_missing_fields() {
        let mut writer = Writer::new();
        writer.write_struct(|s| {
            s.field_string("address", "addr");
            s.field_u64("created_at", 0);
        });
        let bytes = writer.finish();
        let err = match decode_record(&bytes) {
            Ok(_) => panic!("expected decode to fail"),
            Err(err) => err,
        };
        match err {
            DecodeError::InvalidFieldCount { expected, actual } => {
                assert_eq!(expected, FIELD_COUNT);
                assert_eq!(actual, 2);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "pq-crypto")]
    #[test]
    fn record_without_pq_key_matches_legacy() {
        let mut record = sample_record();
        record.pq_pubkey = None;
        let encoded = encode_record(&record);
        let mut reader = Reader::new(&encoded);
        assert_eq!(reader.read_u64().expect("field count"), FIELD_COUNT);
        // Skip first five fields (address, created_at, attest_sig, nonce, version)
        reader.read_string().expect("address key");
        reader.read_string().expect("address");
        reader.read_string().expect("created key");
        reader.read_u64().expect("created_at");
        reader.read_string().expect("attest key");
        let len = reader.read_u64().expect("attest len");
        for _ in 0..len {
            reader.read_u8().expect("attest byte");
        }
        reader.read_string().expect("nonce key");
        reader.read_u64().expect("nonce");
        reader.read_string().expect("version key");
        reader.read_u16().expect("version");
        reader.read_string().expect("pq key");
        assert!(!reader.read_bool().expect("pq absent"));
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0xC0DE_1D11)
    }

    fn random_vec(rng: &mut StdRng, min_len: usize, max_len: usize) -> Vec<u8> {
        debug_assert!(min_len > 0);
        debug_assert!(max_len >= min_len);
        let len = min_len + (rng.next_u32() as usize % (max_len - min_len + 1));
        let mut buf = vec![0u8; len];
        rng.fill_bytes(&mut buf);
        buf
    }

    fn random_ascii(rng: &mut StdRng, min_len: usize, max_len: usize) -> String {
        debug_assert!(min_len > 0);
        debug_assert!(max_len >= min_len);
        let len = min_len + (rng.next_u32() as usize % (max_len - min_len + 1));
        (0..len)
            .map(|_| {
                let v = rng.next_u32() % 36;
                if v < 10 {
                    (b'0' + v as u8) as char
                } else {
                    (b'a' + (v as u8 - 10)) as char
                }
            })
            .collect()
    }

    #[allow(unused_mut)]
    fn random_record(rng: &mut StdRng) -> HandleRecord {
        let mut record = HandleRecord {
            address: random_ascii(rng, 16, 32),
            created_at: rng.next_u64(),
            attest_sig: random_vec(rng, 32, 96),
            nonce: rng.next_u64(),
            version: rng.next_u32() as u16,
            #[cfg(feature = "pq-crypto")]
            pq_pubkey: None,
        };

        #[cfg(feature = "pq-crypto")]
        {
            record.pq_pubkey = if rng.next_u32() % 2 == 0 {
                Some(random_vec(rng, 24, 96))
            } else {
                None
            };
        }

        record
    }

    #[test]
    fn randomized_record_parity() {
        let mut rng = rng();
        for _ in 0..256 {
            let record = random_record(&mut rng);
            let encoded = encode_record(&record);
            let decoded = decode_record(&encoded).expect("decode");
            assert_eq!(record.address, decoded.address);
            assert_eq!(record.created_at, decoded.created_at);
            assert_eq!(record.attest_sig, decoded.attest_sig);
            assert_eq!(record.nonce, decoded.nonce);
            assert_eq!(record.version, decoded.version);
            #[cfg(feature = "pq-crypto")]
            {
                assert_eq!(record.pq_pubkey, decoded.pq_pubkey);
            }
        }
    }

    #[test]
    fn randomized_string_and_u64_parity() {
        let mut rng = rng();
        for _ in 0..64 {
            let string_value = random_ascii(&mut rng, 4, 40);
            let encoded_string = encode_string(&string_value);
            let decoded_string = decode_string(&encoded_string).expect("decode");
            assert_eq!(string_value, decoded_string);

            let value = rng.next_u64();
            let encoded = encode_u64(value);
            let decoded = decode_u64(&encoded).expect("decode");
            assert_eq!(value, decoded);
        }
    }
}

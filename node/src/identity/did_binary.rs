use foundation_serialization::binary_cursor::{Reader, Writer};

use super::{DidAttestationRecord, DidRecord};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};

/// Encode a [`DidRecord`] using the legacy canonical binary layout.
pub fn encode_record(record: &DidRecord) -> Vec<u8> {
    let mut writer = Writer::new();
    write_record(&mut writer, record);
    writer.finish()
}

/// Decode a [`DidRecord`] produced by the canonical binary layout.
pub fn decode_record(bytes: &[u8]) -> binary_struct::Result<DidRecord> {
    let mut reader = Reader::new(bytes);
    let record = read_record(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(record)
}

fn write_record(writer: &mut Writer, record: &DidRecord) {
    writer.write_struct(|s| {
        s.field_string("address", &record.address);
        s.field_string("document", &record.document);
        s.field_with("hash", |w| write_fixed_array(w, &record.hash));
        s.field_u64("nonce", record.nonce);
        s.field_u64("updated_at", record.updated_at);
        s.field_with("public_key", |w| w.write_bytes(&record.public_key));
        if let Some(attestation) = record.remote_attestation.as_ref() {
            s.field_with("remote_attestation", |w| {
                w.write_bool(true);
                write_attestation(w, attestation);
            });
        }
    });
}

fn write_attestation(writer: &mut Writer, attestation: &DidAttestationRecord) {
    writer.write_struct(|s| {
        s.field_string("signer", &attestation.signer);
        s.field_string("signature", &attestation.signature);
    });
}

fn read_record(reader: &mut Reader<'_>) -> binary_struct::Result<DidRecord> {
    let mut address = None;
    let mut document = None;
    let mut hash = None;
    let mut nonce = None;
    let mut updated_at = None;
    let mut public_key = None;
    let mut remote_attestation: Option<Option<DidAttestationRecord>> = None;

    decode_struct(reader, None, |key, reader| match key {
        "address" => {
            let value = reader.read_string()?;
            assign_once(&mut address, value, "address")
        }
        "document" => {
            let value = reader.read_string()?;
            assign_once(&mut document, value, "document")
        }
        "hash" => {
            let value = read_fixed_array::<32>(reader, "hash")?;
            assign_once(&mut hash, value, "hash")
        }
        "nonce" => {
            let value = reader.read_u64()?;
            assign_once(&mut nonce, value, "nonce")
        }
        "updated_at" => {
            let value = reader.read_u64()?;
            assign_once(&mut updated_at, value, "updated_at")
        }
        "public_key" => {
            let value = reader.read_bytes()?;
            assign_once(&mut public_key, value, "public_key")
        }
        "remote_attestation" => {
            if remote_attestation.is_some() {
                return Err(DecodeError::DuplicateField("remote_attestation"));
            }
            let present = reader.read_bool()?;
            if present {
                let value = read_attestation(reader)?;
                remote_attestation = Some(Some(value));
            } else {
                remote_attestation = Some(None);
            }
            Ok(())
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DidRecord {
        address: address.ok_or(DecodeError::MissingField("address"))?,
        document: document.ok_or(DecodeError::MissingField("document"))?,
        hash: hash.ok_or(DecodeError::MissingField("hash"))?,
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        updated_at: updated_at.ok_or(DecodeError::MissingField("updated_at"))?,
        public_key: public_key.ok_or(DecodeError::MissingField("public_key"))?,
        remote_attestation: remote_attestation.unwrap_or(None),
    })
}

fn read_attestation(reader: &mut Reader<'_>) -> binary_struct::Result<DidAttestationRecord> {
    let mut signer = None;
    let mut signature = None;

    decode_struct(reader, Some(2), |key, reader| match key {
        "signer" => {
            let value = reader.read_string()?;
            assign_once(&mut signer, value, "signer")
        }
        "signature" => {
            let value = reader.read_string()?;
            assign_once(&mut signature, value, "signature")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DidAttestationRecord {
        signer: signer.ok_or(DecodeError::MissingField("signer"))?,
        signature: signature.ok_or(DecodeError::MissingField("signature"))?,
    })
}

fn write_fixed_array<const N: usize>(writer: &mut Writer, value: &[u8; N]) {
    writer.write_bytes(value);
}

fn read_fixed_array<const N: usize>(
    reader: &mut Reader<'_>,
    field: &'static str,
) -> binary_struct::Result<[u8; N]> {
    let bytes = reader.read_bytes()?;
    if bytes.len() != N {
        return Err(DecodeError::InvalidFieldValue {
            field,
            reason: format!("expected {N} bytes got {}", bytes.len()),
        });
    }
    let mut array = [0u8; N];
    array.copy_from_slice(&bytes);
    Ok(array)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::did::DidAttestationRecord;
    use foundation_serialization::binary_cursor::Writer;
    use rand::{rngs::StdRng, RngCore};

    const DID_RECORD_FIXTURE: &[u8] = &[
        7, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 97, 100, 100, 114, 101, 115, 115, 15, 0, 0,
        0, 0, 0, 0, 0, 100, 105, 100, 58, 98, 108, 111, 99, 107, 58, 97, 108, 105, 99, 101, 8, 0,
        0, 0, 0, 0, 0, 0, 100, 111, 99, 117, 109, 101, 110, 116, 14, 0, 0, 0, 0, 0, 0, 0, 123, 34,
        115, 101, 114, 118, 105, 99, 101, 34, 58, 91, 93, 125, 4, 0, 0, 0, 0, 0, 0, 0, 104, 97,
        115, 104, 32, 0, 0, 0, 0, 0, 0, 0, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170,
        170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170, 170,
        170, 170, 170, 5, 0, 0, 0, 0, 0, 0, 0, 110, 111, 110, 99, 101, 42, 0, 0, 0, 0, 0, 0, 0, 10,
        0, 0, 0, 0, 0, 0, 0, 117, 112, 100, 97, 116, 101, 100, 95, 97, 116, 64, 226, 1, 0, 0, 0, 0,
        0, 10, 0, 0, 0, 0, 0, 0, 0, 112, 117, 98, 108, 105, 99, 95, 107, 101, 121, 5, 0, 0, 0, 0,
        0, 0, 0, 1, 2, 3, 4, 5, 18, 0, 0, 0, 0, 0, 0, 0, 114, 101, 109, 111, 116, 101, 95, 97, 116,
        116, 101, 115, 116, 97, 116, 105, 111, 110, 1, 2, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
        0, 115, 105, 103, 110, 101, 114, 8, 0, 0, 0, 0, 0, 0, 0, 100, 101, 97, 100, 98, 101, 101,
        102, 9, 0, 0, 0, 0, 0, 0, 0, 115, 105, 103, 110, 97, 116, 117, 114, 101, 8, 0, 0, 0, 0, 0,
        0, 0, 99, 97, 102, 101, 98, 97, 98, 101,
    ];

    fn with_first_party_only_env<R>(value: Option<&str>, f: impl FnOnce() -> R) -> R {
        static GUARD: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let lock = GUARD
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env guard");

        let original = std::env::var("FIRST_PARTY_ONLY").ok();
        match value {
            Some(v) => std::env::set_var("FIRST_PARTY_ONLY", v),
            None => std::env::remove_var("FIRST_PARTY_ONLY"),
        }

        let result = f();

        match original {
            Some(v) => std::env::set_var("FIRST_PARTY_ONLY", v),
            None => std::env::remove_var("FIRST_PARTY_ONLY"),
        }

        drop(lock);
        result
    }

    fn sample_record() -> DidRecord {
        DidRecord {
            address: "did:block:alice".into(),
            document: "{\"service\":[]}".into(),
            hash: [0xAA; 32],
            nonce: 42,
            updated_at: 123_456,
            public_key: vec![1, 2, 3, 4, 5],
            remote_attestation: Some(DidAttestationRecord {
                signer: "deadbeef".into(),
                signature: "cafebabe".into(),
            }),
        }
    }

    #[test]
    fn round_trip_with_attestation() {
        let record = sample_record();
        let encoded = encode_record(&record);
        let decoded = decode_record(&encoded).expect("decode");
        assert_eq!(record, decoded);
    }

    #[test]
    fn round_trip_without_attestation() {
        let mut record = sample_record();
        record.remote_attestation = None;
        let encoded = encode_record(&record);
        let decoded = decode_record(&encoded).expect("decode");
        assert_eq!(record, decoded);
    }

    #[test]
    fn compatibility_with_legacy_codec() {
        let record = sample_record();
        let encoded = encode_record(&record);
        let mut reader = Reader::new(&encoded);
        assert_eq!(reader.read_u64().expect("field count"), 7);
        assert_eq!(reader.read_string().expect("address key"), "address");
        assert_eq!(reader.read_string().expect("address"), record.address);
        assert_eq!(reader.read_string().expect("document key"), "document");
        assert_eq!(reader.read_string().expect("document"), record.document);
        assert_eq!(reader.read_string().expect("hash key"), "hash");
        assert_eq!(reader.read_u64().expect("hash len"), 32);
        for byte in record.hash {
            assert_eq!(reader.read_u8().expect("hash byte"), byte);
        }
        assert_eq!(reader.read_string().expect("nonce key"), "nonce");
        assert_eq!(reader.read_u64().expect("nonce"), record.nonce);
        assert_eq!(reader.read_string().expect("updated_at key"), "updated_at");
        assert_eq!(reader.read_u64().expect("updated_at"), record.updated_at);
        assert_eq!(reader.read_string().expect("public_key key"), "public_key");
        assert_eq!(
            reader.read_u64().expect("public key len"),
            record.public_key.len() as u64
        );
        for byte in &record.public_key {
            assert_eq!(reader.read_u8().expect("public key byte"), *byte);
        }
        assert_eq!(
            reader.read_string().expect("attestation key"),
            "remote_attestation"
        );
        assert!(reader.read_bool().expect("attestation present"));
        assert_eq!(reader.read_u64().expect("attestation fields"), 2);
        assert_eq!(reader.read_string().expect("signer key"), "signer");
        assert_eq!(
            reader.read_string().expect("signer"),
            record.remote_attestation.as_ref().unwrap().signer
        );
        assert_eq!(reader.read_string().expect("signature key"), "signature");
        assert_eq!(
            reader.read_string().expect("signature"),
            record.remote_attestation.as_ref().unwrap().signature
        );

        let decoded = decode_record(&encoded).expect("manual decode");
        assert_eq!(record, decoded);
    }

    #[test]
    fn compatibility_without_attestation() {
        let mut record = sample_record();
        record.remote_attestation = None;

        let encoded = encode_record(&record);
        let mut reader = Reader::new(&encoded);
        assert_eq!(reader.read_u64().expect("field count"), 6);
        assert_eq!(reader.read_string().expect("address key"), "address");
        assert_eq!(reader.read_string().expect("address"), record.address);
        assert_eq!(reader.read_string().expect("document key"), "document");
        assert_eq!(reader.read_string().expect("document"), record.document);
        assert_eq!(reader.read_string().expect("hash key"), "hash");
        assert_eq!(reader.read_u64().expect("hash len"), 32);
        for byte in record.hash {
            assert_eq!(reader.read_u8().expect("hash byte"), byte);
        }
        assert_eq!(reader.read_string().expect("nonce key"), "nonce");
        assert_eq!(reader.read_u64().expect("nonce"), record.nonce);
        assert_eq!(reader.read_string().expect("updated key"), "updated_at");
        assert_eq!(reader.read_u64().expect("updated_at"), record.updated_at);
        assert_eq!(reader.read_string().expect("public key"), "public_key");
        assert_eq!(
            reader.read_u64().expect("public key len"),
            record.public_key.len() as u64
        );
        for byte in &record.public_key {
            assert_eq!(reader.read_u8().expect("public key byte"), *byte);
        }

        let decoded = decode_record(&encoded).expect("manual decode");
        assert_eq!(record, decoded);
    }

    #[test]
    fn did_record_roundtrip_matches_fixture() {
        let record = sample_record();
        let encoded = encode_record(&record);
        if DID_RECORD_FIXTURE.is_empty() {
            panic!("fixture pending: {:?}", encoded);
        }
        assert_eq!(encoded, DID_RECORD_FIXTURE);

        let decoded = decode_record(DID_RECORD_FIXTURE).expect("decode fixture");
        assert_eq!(decoded, record);
    }

    #[test]
    fn did_record_roundtrip_respects_first_party_only_flag() {
        let record = sample_record();
        for flag in [Some("1"), Some("0"), None] {
            with_first_party_only_env(flag, || {
                let encoded = encode_record(&record);
                let decoded = decode_record(&encoded).expect("decode under flag");
                assert_eq!(decoded, record);
            });
        }
    }

    #[test]
    fn rejects_incorrect_hash_length() {
        let mut writer = Writer::new();
        writer.write_struct(|s| {
            s.field_string("address", "addr");
            s.field_string("document", "doc");
            s.field_with("hash", |w| w.write_bytes(&[1, 2, 3]));
            s.field_u64("nonce", 0);
            s.field_u64("updated_at", 0);
            s.field_with("public_key", |w| w.write_bytes(&[]));
            s.field_with("remote_attestation", |w| w.write_bool(false));
        });
        let bytes = writer.finish();

        let err = decode_record(&bytes).expect_err("decode should fail");
        match err {
            DecodeError::InvalidFieldValue { field, .. } => assert_eq!(field, "hash"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0xD1D5_A11E)
    }

    fn random_ascii(rng: &mut StdRng, min_len: usize, max_len: usize) -> String {
        debug_assert!(min_len > 0);
        debug_assert!(max_len >= min_len);
        let span = max_len - min_len + 1;
        let len = min_len + (rng.next_u32() as usize % span);
        (0..len)
            .map(|_| {
                let value = rng.next_u32() % 62;
                match value {
                    0..=9 => (b'0' + value as u8) as char,
                    10..=35 => (b'a' + (value as u8 - 10)) as char,
                    _ => (b'A' + (value as u8 - 36)) as char,
                }
            })
            .collect()
    }

    fn random_record(rng: &mut StdRng) -> DidRecord {
        let mut hash = [0u8; 32];
        rng.fill_bytes(&mut hash);
        let mut public_key = vec![0u8; 32];
        rng.fill_bytes(&mut public_key);
        let mut record = DidRecord {
            address: format!("did:block:{}", random_ascii(rng, 6, 18)),
            document: format!("{{\"service\":\"{}\"}}", random_ascii(rng, 4, 24)),
            hash,
            nonce: rng.next_u64(),
            updated_at: rng.next_u64(),
            public_key,
            remote_attestation: None,
        };

        if rng.next_u32() % 3 == 0 {
            record.remote_attestation = Some(DidAttestationRecord {
                signer: random_ascii(rng, 8, 32),
                signature: random_ascii(rng, 16, 48),
            });
        }

        record
    }

    #[test]
    fn randomized_round_trip_parity() {
        let mut rng = rng();
        for _ in 0..256 {
            let record = random_record(&mut rng);
            let encoded = encode_record(&record);
            let decoded = decode_record(&encoded).expect("decode");
            assert_eq!(record, decoded);

            let legacy = encode_via_writer(&record);
            assert_eq!(legacy, encoded);

            let legacy_decoded = decode_record(&legacy).expect("legacy decode");
            assert_eq!(legacy_decoded, record);
        }
    }

    fn encode_via_writer(record: &DidRecord) -> Vec<u8> {
        let mut writer = Writer::new();
        super::write_record(&mut writer, record);
        writer.finish()
    }
}

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
    use crate::util::binary_codec;
    use foundation_serialization::binary_cursor::Writer;
    use rand::{rngs::StdRng, RngCore};

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
        let legacy = binary_codec::serialize(&record).expect("legacy encode");
        let manual = encode_record(&record);
        assert_eq!(legacy, manual);

        let decoded = decode_record(&legacy).expect("manual decode");
        assert_eq!(record, decoded);
    }

    #[test]
    fn compatibility_without_attestation() {
        let mut record = sample_record();
        record.remote_attestation = None;

        let legacy = binary_codec::serialize(&record).expect("legacy encode");
        let manual = encode_record(&record);
        assert_eq!(legacy, manual);

        let decoded = decode_record(&legacy).expect("manual decode");
        assert_eq!(record, decoded);
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

            let legacy = binary_codec::serialize(&record).expect("legacy encode");
            assert_eq!(legacy, encoded);

            let legacy_decoded = decode_record(&legacy).expect("legacy decode");
            assert_eq!(legacy_decoded, record);
        }
    }
}

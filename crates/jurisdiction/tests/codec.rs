use foundation_serialization::binary_cursor::{Reader, Writer};
use jurisdiction::codec::{
    decode_policy_pack, decode_signed_pack, encode_policy_pack, encode_signed_pack, CodecError,
};
use jurisdiction::{PolicyPack, SignedPack};

fn sample_pack() -> PolicyPack {
    PolicyPack {
        region: "US-NY".into(),
        consent_required: true,
        features: vec!["wallet".into(), "storage".into()],
        parent: Some("US".into()),
    }
}

fn sample_signed_pack() -> SignedPack {
    SignedPack {
        pack: sample_pack(),
        signature: vec![1, 2, 3, 4],
    }
}

#[test]
fn policy_pack_binary_roundtrip() {
    let pack = sample_pack();
    let encoded = encode_policy_pack(&pack);
    let decoded = decode_policy_pack(&encoded).expect("decode policy pack");
    assert_eq!(decoded, pack);
}

#[test]
fn signed_pack_binary_roundtrip() {
    let signed = sample_signed_pack();
    let encoded = encode_signed_pack(&signed);
    let decoded = decode_signed_pack(&encoded).expect("decode signed pack");
    assert_eq!(decoded, signed);
}

#[test]
fn decode_policy_pack_detects_missing_field() {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        struct_writer.field_bool("consent_required", true);
        struct_writer.field_vec_with("features", &["wallet".to_string()], |w, value| {
            w.write_string(value);
        });
    });

    let err = decode_policy_pack(&writer.finish()).expect_err("missing region should error");
    assert!(matches!(err, CodecError::MissingField("region")));
}

#[test]
fn decode_signed_pack_detects_trailing_bytes() {
    let signed = sample_signed_pack();
    let mut bytes = encode_signed_pack(&signed);
    bytes.extend_from_slice(&[0, 1, 2]);

    let err = decode_signed_pack(&bytes).expect_err("trailing bytes should fail");
    assert!(matches!(err, CodecError::TrailingBytes(3)));
}

#[test]
fn decode_policy_pack_rejects_duplicate_fields() {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        struct_writer.field_string("region", "US");
        struct_writer.field_string("region", "EU");
        struct_writer.field_bool("consent_required", true);
        struct_writer.field_vec_with("features", &["wallet".to_string()], |w, value| {
            w.write_string(value);
        });
    });
    let err = decode_policy_pack(&writer.finish()).expect_err("duplicate region should error");
    assert!(matches!(err, CodecError::DuplicateField("region")));
}

#[test]
fn decode_signed_pack_rejects_unexpected_field() {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("pack", |writer| {
            writer.write_struct(|nested| {
                nested.field_string("region", "US");
                nested.field_bool("consent_required", false);
                nested.field_vec_with("features", &["wallet".to_string()], |w, value| {
                    w.write_string(value);
                });
                nested.field_option_string("parent", None);
            });
        });
        struct_writer.field_bytes("signature", &[1, 2, 3]);
        struct_writer.field_string("extra", "forbidden");
    });

    let err = decode_signed_pack(&writer.finish()).expect_err("unexpected field should fail");
    assert!(matches!(err, CodecError::UnexpectedField(field) if field == "extra"));
}

#[test]
fn signed_pack_detects_missing_signature() {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("pack", |writer| {
            writer.write_struct(|nested| {
                nested.field_string("region", "US");
                nested.field_bool("consent_required", true);
                nested.field_vec_with("features", &["wallet".to_string()], |w, value| {
                    w.write_string(value);
                });
                nested.field_option_string("parent", None);
            });
        });
    });

    let err = decode_signed_pack(&writer.finish()).expect_err("missing signature should error");
    assert!(matches!(err, CodecError::MissingField("signature")));
}

#[test]
fn encode_signed_pack_matches_manual_construction() {
    let signed = sample_signed_pack();
    let encoded = encode_signed_pack(&signed);
    let mut reader = Reader::new(&encoded);

    let mut saw_fields = Vec::new();
    reader
        .read_struct_with(|key, nested| -> Result<(), CodecError> {
            saw_fields.push(key.to_owned());
            if key == "pack" {
                nested
                    .read_struct_with(|pack_key, pack_reader| -> Result<(), CodecError> {
                        match pack_key {
                            "region" => {
                                assert_eq!(pack_reader.read_string().unwrap(), "US-NY");
                            }
                            "consent_required" => {
                                assert!(pack_reader.read_bool().unwrap());
                            }
                            "features" => {
                                let values = pack_reader
                                    .read_vec_with(|r| r.read_string().map_err(CodecError::from))
                                    .unwrap();
                                assert_eq!(
                                    values,
                                    vec!["wallet".to_string(), "storage".to_string()]
                                );
                            }
                            "parent" => {
                                let parent = pack_reader
                                    .read_option_with(|r| r.read_string().map_err(CodecError::from))
                                    .unwrap();
                                assert_eq!(parent, Some("US".to_string()));
                            }
                            other => panic!("unexpected pack field {other}"),
                        }
                        Ok(())
                    })
                    .unwrap();
            } else if key == "signature" {
                let bytes = nested
                    .read_vec_with(|r| r.read_u8().map_err(CodecError::from))
                    .unwrap();
                assert_eq!(bytes, vec![1, 2, 3, 4]);
            } else {
                panic!("unexpected top-level field {key}");
            }
            Ok(())
        })
        .unwrap();

    assert_eq!(
        saw_fields,
        vec!["pack".to_string(), "signature".to_string()]
    );
}

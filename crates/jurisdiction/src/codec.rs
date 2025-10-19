use crate::{Change, PolicyDiff, PolicyPack, SignedPack};
use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};
use std::fmt;

#[derive(Debug)]
pub enum CodecError {
    Binary(CursorError),
    MissingField(&'static str),
    UnknownField(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Binary(err) => write!(f, "binary decode error: {err}"),
            CodecError::MissingField(field) => write!(f, "missing field: {field}"),
            CodecError::UnknownField(field) => write!(f, "unknown field encountered: {field}"),
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CodecError::Binary(err) => Some(err),
            CodecError::MissingField(_) => None,
            CodecError::UnknownField(_) => None,
        }
    }
}

impl From<CursorError> for CodecError {
    fn from(err: CursorError) -> Self {
        CodecError::Binary(err)
    }
}

pub type CodecResult<T> = Result<T, CodecError>;

pub fn encode_policy_pack(pack: &PolicyPack) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        struct_writer.field_string("region", &pack.region);
        struct_writer.field_bool("consent_required", pack.consent_required);
        struct_writer.field_vec_with("features", &pack.features, |writer, feature| {
            writer.write_string(feature);
        });
        struct_writer.field_option_string("parent", pack.parent.as_deref());
    });
    writer.finish()
}

pub fn decode_policy_pack(bytes: &[u8]) -> CodecResult<PolicyPack> {
    let mut reader = Reader::new(bytes);
    let mut region: Option<String> = None;
    let mut consent_required = false;
    let mut consent_set = false;
    let mut features: Option<Vec<String>> = None;
    let mut parent: Option<Option<String>> = None;

    reader.read_struct_with(|key, reader| -> CodecResult<()> {
        match key {
            "region" => {
                region = Some(reader.read_string()?);
            }
            "consent_required" => {
                consent_required = reader.read_bool()?;
                consent_set = true;
            }
            "features" => {
                let values = reader.read_vec_with(|reader| reader.read_string())?;
                features = Some(values);
            }
            "parent" => {
                let value = reader.read_option_with(|reader| reader.read_string())?;
                parent = Some(value);
            }
            _ => return Err(CodecError::UnknownField(key.to_owned())),
        }
        Ok(())
    })?;

    let region = region.ok_or(CodecError::MissingField("region"))?;
    if !consent_set {
        return Err(CodecError::MissingField("consent_required"));
    }
    let features = features.unwrap_or_default();
    let parent = parent.unwrap_or(None);

    Ok(PolicyPack {
        region,
        consent_required,
        features,
        parent,
    })
}

pub fn encode_signed_pack(pack: &SignedPack) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("pack", |writer| {
            let encoded = encode_policy_pack(&pack.pack);
            writer.write_bytes(&encoded);
        });
        struct_writer.field_vec_with("signature", &pack.signature, |writer, byte| {
            writer.write_u8(*byte);
        });
    });
    writer.finish()
}

pub fn decode_signed_pack(bytes: &[u8]) -> CodecResult<SignedPack> {
    let mut reader = Reader::new(bytes);
    let mut pack: Option<PolicyPack> = None;
    let mut signature: Option<Vec<u8>> = None;

    reader.read_struct_with(|key, reader| -> CodecResult<()> {
        match key {
            "pack" => {
                let encoded = reader.read_bytes()?;
                pack = Some(decode_policy_pack(&encoded)?);
            }
            "signature" => {
                signature = Some(reader.read_vec_with(|reader| reader.read_u8())?);
            }
            _ => return Err(CodecError::UnknownField(key.to_owned())),
        }
        Ok(())
    })?;

    let pack = pack.ok_or(CodecError::MissingField("pack"))?;
    let signature = signature.ok_or(CodecError::MissingField("signature"))?;

    Ok(SignedPack { pack, signature })
}

pub fn encode_policy_diff(diff: &PolicyDiff) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        if let Some(change) = &diff.consent_required {
            struct_writer.field_with("consent_required", |writer| {
                writer.write_bool(change.old);
                writer.write_bool(change.new);
            });
        }
        if let Some(change) = &diff.features {
            struct_writer.field_with("features", |writer| {
                writer.write_vec_with(&change.old, |writer, value| writer.write_string(value));
                writer.write_vec_with(&change.new, |writer, value| writer.write_string(value));
            });
        }
    });
    writer.finish()
}

pub fn decode_policy_diff(bytes: &[u8]) -> CodecResult<PolicyDiff> {
    let mut reader = Reader::new(bytes);
    let mut diff = PolicyDiff::default();

    reader.read_struct_with(|key, reader| -> CodecResult<()> {
        match key {
            "consent_required" => {
                let old = reader.read_bool()?;
                let new = reader.read_bool()?;
                diff.consent_required = Some(Change::new(old, new));
            }
            "features" => {
                let old = reader.read_vec_with(|reader| reader.read_string())?;
                let new = reader.read_vec_with(|reader| reader.read_string())?;
                diff.features = Some(Change::new(old, new));
            }
            _ => return Err(CodecError::UnknownField(key.to_owned())),
        }
        Ok(())
    })?;

    Ok(diff)
}

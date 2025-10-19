use std::fmt;

use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::{PolicyPack, SignedPack};

/// Result type returned by the jurisdiction codecs.
pub type Result<T> = std::result::Result<T, CodecError>;

/// Errors raised while encoding or decoding policy packs.
#[derive(Debug)]
pub enum CodecError {
    /// Underlying cursor error produced by the binary helpers.
    Cursor(CursorError),
    /// A required field was missing from the payload.
    MissingField(&'static str),
    /// Encountered a field multiple times while decoding.
    DuplicateField(&'static str),
    /// Encountered a field that is not recognised by the codec.
    UnexpectedField(String),
    /// Trailing bytes remained after decoding finished.
    TrailingBytes(usize),
}

impl From<CursorError> for CodecError {
    fn from(err: CursorError) -> Self {
        CodecError::Cursor(err)
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Cursor(err) => write!(f, "{err}"),
            CodecError::MissingField(field) => write!(f, "missing required field {field}"),
            CodecError::DuplicateField(field) => write!(f, "duplicate field {field}"),
            CodecError::UnexpectedField(field) => write!(f, "unexpected field {field}"),
            CodecError::TrailingBytes(count) => {
                write!(f, "{count} trailing bytes remaining after decode")
            }
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CodecError::Cursor(err) => Some(err),
            _ => None,
        }
    }
}

fn ensure_absent<T>(slot: &mut Option<T>, field: &'static str) -> Result<()> {
    if slot.is_some() {
        return Err(CodecError::DuplicateField(field));
    }
    Ok(())
}

fn finish_pack_fields(pack: Option<PolicyPackFields>) -> Result<PolicyPack> {
    let fields = pack.ok_or(CodecError::MissingField("pack"))?;
    Ok(PolicyPack {
        region: fields.region.ok_or(CodecError::MissingField("region"))?,
        consent_required: fields
            .consent_required
            .ok_or(CodecError::MissingField("consent_required"))?,
        features: fields
            .features
            .ok_or(CodecError::MissingField("features"))?,
        parent: fields.parent.unwrap_or(None),
    })
}

struct PolicyPackFields {
    region: Option<String>,
    consent_required: Option<bool>,
    features: Option<Vec<String>>,
    parent: Option<Option<String>>,
}

fn write_policy_pack_fields(
    writer: &mut foundation_serialization::binary_cursor::StructWriter,
    pack: &PolicyPack,
) {
    writer.field_string("region", &pack.region);
    writer.field_bool("consent_required", pack.consent_required);
    writer.field_vec_with("features", &pack.features, |w, value| w.write_string(value));
    writer.field_option_string("parent", pack.parent.as_deref());
}

fn read_policy_pack_fields(reader: &mut Reader<'_>) -> Result<PolicyPack> {
    let mut fields = PolicyPackFields {
        region: None,
        consent_required: None,
        features: None,
        parent: None,
    };

    reader.read_struct_with(|key, nested| {
        match key {
            "region" => {
                ensure_absent(&mut fields.region, "region")?;
                fields.region = Some(nested.read_string().map_err(CodecError::from)?);
            }
            "consent_required" => {
                ensure_absent(&mut fields.consent_required, "consent_required")?;
                fields.consent_required = Some(nested.read_bool().map_err(CodecError::from)?);
            }
            "features" => {
                ensure_absent(&mut fields.features, "features")?;
                let values = nested.read_vec_with(|r| r.read_string().map_err(CodecError::from))?;
                fields.features = Some(values);
            }
            "parent" => {
                ensure_absent(&mut fields.parent, "parent")?;
                let value =
                    nested.read_option_with(|r| r.read_string().map_err(CodecError::from))?;
                fields.parent = Some(value);
            }
            other => return Err(CodecError::UnexpectedField(other.to_owned())),
        }
        Ok(())
    })?;

    finish_pack_fields(Some(fields))
}

/// Encode a [`PolicyPack`] into the first-party binary representation.
pub fn encode_policy_pack(pack: &PolicyPack) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        write_policy_pack_fields(struct_writer, pack);
    });
    writer.finish()
}

/// Decode a [`PolicyPack`] from the first-party binary representation.
pub fn decode_policy_pack(bytes: &[u8]) -> Result<PolicyPack> {
    let mut reader = Reader::new(bytes);
    let pack = read_policy_pack_fields(&mut reader)?;
    if reader.remaining() != 0 {
        return Err(CodecError::TrailingBytes(reader.remaining()));
    }
    Ok(pack)
}

/// Encode a [`SignedPack`] into the first-party binary representation.
pub fn encode_signed_pack(pack: &SignedPack) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("pack", |writer| {
            writer.write_struct(|nested| {
                write_policy_pack_fields(nested, &pack.pack);
            });
        });
        struct_writer.field_vec_with("signature", &pack.signature, |w, byte| w.write_u8(*byte));
    });
    writer.finish()
}

/// Decode a [`SignedPack`] from the first-party binary representation.
pub fn decode_signed_pack(bytes: &[u8]) -> Result<SignedPack> {
    let mut reader = Reader::new(bytes);
    let mut pack: Option<PolicyPack> = None;
    let mut signature: Option<Vec<u8>> = None;

    reader.read_struct_with(|key, nested| {
        match key {
            "pack" => {
                ensure_absent(&mut pack, "pack")?;
                let decoded = read_policy_pack_fields(nested)?;
                pack = Some(decoded);
            }
            "signature" => {
                ensure_absent(&mut signature, "signature")?;
                let bytes = nested.read_vec_with(|r| r.read_u8().map_err(CodecError::from))?;
                signature = Some(bytes);
            }
            other => return Err(CodecError::UnexpectedField(other.to_owned())),
        }
        Ok(())
    })?;

    if reader.remaining() != 0 {
        return Err(CodecError::TrailingBytes(reader.remaining()));
    }

    let pack = pack.ok_or(CodecError::MissingField("pack"))?;
    let signature = signature.ok_or(CodecError::MissingField("signature"))?;

    Ok(SignedPack { pack, signature })
}

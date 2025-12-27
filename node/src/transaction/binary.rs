use std::convert::TryFrom;
use std::fmt;

use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::transaction::{
    BlobTx, FeeLane, RawTxPayload, SignedTransaction, TxSignature, TxVersion,
};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};

/// Result alias for transaction encoding helpers.
pub type EncodeResult<T> = Result<T, EncodeError>;

/// Error returned by manual transaction encoders.
#[derive(Debug)]
pub enum EncodeError {
    /// Collection length exceeded the representable range.
    LengthOverflow(&'static str),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::LengthOverflow(field) => {
                write!(f, "{field} length exceeds u64::MAX")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Encode a [`RawTxPayload`] using the canonical legacy layout.
pub fn encode_raw_payload(payload: &RawTxPayload) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(128);
    write_raw_payload(&mut writer, payload)?;
    Ok(writer.finish())
}

/// Decode a [`RawTxPayload`] from bytes produced by [`encode_raw_payload`].
pub fn decode_raw_payload(bytes: &[u8]) -> binary_struct::Result<RawTxPayload> {
    let mut reader = Reader::new(bytes);
    let payload = read_raw_payload(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(payload)
}

/// Encode a [`SignedTransaction`] into the canonical binary layout.
pub fn encode_signed_transaction(tx: &SignedTransaction) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(512);
    write_signed_transaction(&mut writer, tx)?;
    Ok(writer.finish())
}

/// Decode a [`SignedTransaction`] produced by [`encode_signed_transaction`].
pub fn decode_signed_transaction(bytes: &[u8]) -> binary_struct::Result<SignedTransaction> {
    let mut reader = Reader::new(bytes);
    let tx = read_signed_transaction(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(tx)
}

/// Encode a [`BlobTx`] using the canonical binary layout.
pub fn encode_blob_tx(tx: &BlobTx) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(128);
    write_blob_tx(&mut writer, tx)?;
    Ok(writer.finish())
}

/// Decode a [`BlobTx`] from bytes produced by [`encode_blob_tx`].
pub fn decode_blob_tx(bytes: &[u8]) -> binary_struct::Result<BlobTx> {
    let mut reader = Reader::new(bytes);
    let tx = read_blob_tx(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(tx)
}

fn write_raw_payload(writer: &mut Writer, payload: &RawTxPayload) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_string("from_", &payload.from_);
        struct_writer.field_string("to", &payload.to);
        struct_writer.field_u64("amount_consumer", payload.amount_consumer);
        struct_writer.field_u64("amount_industrial", payload.amount_industrial);
        struct_writer.field_u64("fee", payload.fee);
        struct_writer.field_u8("pct", payload.pct);
        struct_writer.field_u64("nonce", payload.nonce);
        struct_writer.field_with("memo", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_bytes(field_writer, &payload.memo, "memo") {
                    result = Err(err);
                }
            }
        });
    });
    result
}

pub(crate) fn read_raw_payload(reader: &mut Reader<'_>) -> binary_struct::Result<RawTxPayload> {
    let mut from_ = None;
    let mut to = None;
    let mut amount_consumer = None;
    let mut amount_industrial = None;
    let mut fee = None;
    let mut pct = None;
    let mut nonce = None;
    let mut memo = None;

    decode_struct(reader, Some(8), |key, reader| match key {
        "from_" => assign_once(&mut from_, reader.read_string()?, "from_"),
        "to" => assign_once(&mut to, reader.read_string()?, "to"),
        "amount_consumer" => {
            assign_once(&mut amount_consumer, reader.read_u64()?, "amount_consumer")
        }
        "amount_industrial" => assign_once(
            &mut amount_industrial,
            reader.read_u64()?,
            "amount_industrial",
        ),
        "fee" => assign_once(&mut fee, reader.read_u64()?, "fee"),
        "pct" => assign_once(&mut pct, reader.read_u8()?, "pct"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "memo" => assign_once(&mut memo, reader.read_bytes()?, "memo"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(RawTxPayload {
        from_: from_.ok_or(DecodeError::MissingField("from_"))?,
        to: to.ok_or(DecodeError::MissingField("to"))?,
        amount_consumer: amount_consumer.ok_or(DecodeError::MissingField("amount_consumer"))?,
        amount_industrial: amount_industrial
            .ok_or(DecodeError::MissingField("amount_industrial"))?,
        fee: fee.ok_or(DecodeError::MissingField("fee"))?,
        pct: pct.ok_or(DecodeError::MissingField("pct"))?,
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        memo: memo.unwrap_or_default(),
    })
}

pub(crate) fn write_signed_transaction(
    writer: &mut Writer,
    tx: &SignedTransaction,
) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("payload", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_raw_payload(field_writer, &tx.payload) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("public_key", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_bytes(field_writer, &tx.public_key, "public_key") {
                    result = Err(err);
                }
            }
        });
        #[cfg(feature = "quantum")]
        {
            struct_writer.field_with("dilithium_public_key", |field_writer| {
                if result.is_ok() {
                    if let Err(err) = write_bytes(
                        field_writer,
                        &tx.dilithium_public_key,
                        "dilithium_public_key",
                    ) {
                        result = Err(err);
                    }
                }
            });
        }
        struct_writer.field_with("signature", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_signature(field_writer, &tx.signature) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_u64("tip", tx.tip);
        struct_writer.field_with("signer_pubkeys", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_vec(
                    field_writer,
                    &tx.signer_pubkeys,
                    "signer_pubkeys",
                    |writer, value| write_bytes(writer, value, "signer_pubkey"),
                ) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("aggregate_signature", |field_writer| {
            if result.is_ok() {
                if let Err(err) =
                    write_bytes(field_writer, &tx.aggregate_signature, "aggregate_signature")
                {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_u8("threshold", tx.threshold);
        struct_writer.field_with("lane", |field_writer| {
            write_fee_lane(field_writer, tx.lane);
        });
        struct_writer.field_with("version", |field_writer| {
            write_tx_version(field_writer, tx.version);
        });
    });
    result
}

pub(crate) fn read_signed_transaction(
    reader: &mut Reader<'_>,
) -> binary_struct::Result<SignedTransaction> {
    let mut payload = None;
    let mut public_key = None;
    #[cfg(feature = "quantum")]
    let mut dilithium_public_key = None;
    let mut signature = None;
    let mut tip = None;
    let mut signer_pubkeys = None;
    let mut aggregate_signature = None;
    let mut threshold = None;
    let mut lane = None;
    let mut version = None;

    decode_struct(reader, None, |key, reader| match key {
        "payload" => assign_once(&mut payload, read_raw_payload(reader)?, "payload"),
        "public_key" => assign_once(&mut public_key, reader.read_bytes()?, "public_key"),
        #[cfg(feature = "quantum")]
        "dilithium_public_key" => assign_once(
            &mut dilithium_public_key,
            reader.read_bytes()?,
            "dilithium_public_key",
        ),
        "signature" => assign_once(&mut signature, read_signature(reader)?, "signature"),
        "tip" => assign_once(&mut tip, reader.read_u64()?, "tip"),
        "signer_pubkeys" => {
            let values = read_vec(reader, "signer_pubkeys", |reader| {
                reader.read_bytes().map_err(DecodeError::from)
            })?;
            assign_once(&mut signer_pubkeys, values, "signer_pubkeys")
        }
        "aggregate_signature" => assign_once(
            &mut aggregate_signature,
            reader.read_bytes()?,
            "aggregate_signature",
        ),
        "threshold" => assign_once(&mut threshold, reader.read_u8()?, "threshold"),
        "lane" => assign_once(&mut lane, read_fee_lane(reader)?, "lane"),
        "version" => assign_once(&mut version, read_tx_version(reader)?, "version"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(SignedTransaction {
        payload: payload.ok_or(DecodeError::MissingField("payload"))?,
        public_key: public_key.unwrap_or_default(),
        #[cfg(feature = "quantum")]
        dilithium_public_key: dilithium_public_key.unwrap_or_default(),
        signature: signature.unwrap_or_default(),
        tip: tip.unwrap_or_default(),
        signer_pubkeys: signer_pubkeys.unwrap_or_default(),
        aggregate_signature: aggregate_signature.unwrap_or_default(),
        threshold: threshold.unwrap_or_default(),
        lane: lane.unwrap_or(FeeLane::Consumer),
        version: version.unwrap_or_default(),
    })
}

fn write_signature(writer: &mut Writer, signature: &TxSignature) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("ed25519", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_bytes(field_writer, &signature.ed25519, "ed25519") {
                    result = Err(err);
                }
            }
        });
        #[cfg(feature = "quantum")]
        {
            struct_writer.field_with("dilithium", |field_writer| {
                if result.is_ok() {
                    if let Err(err) = write_bytes(field_writer, &signature.dilithium, "dilithium") {
                        result = Err(err);
                    }
                }
            });
        }
    });
    result
}

fn read_signature(reader: &mut Reader<'_>) -> binary_struct::Result<TxSignature> {
    let mut ed25519 = None;
    #[cfg(feature = "quantum")]
    let mut dilithium = None;

    decode_struct(reader, None, |key, reader| match key {
        "ed25519" => assign_once(&mut ed25519, reader.read_bytes()?, "ed25519"),
        #[cfg(feature = "quantum")]
        "dilithium" => assign_once(&mut dilithium, reader.read_bytes()?, "dilithium"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(TxSignature {
        ed25519: ed25519.unwrap_or_default(),
        #[cfg(feature = "quantum")]
        dilithium: dilithium.unwrap_or_default(),
    })
}

fn write_fee_lane(writer: &mut Writer, lane: FeeLane) {
    let idx = match lane {
        FeeLane::Consumer => 0,
        FeeLane::Industrial => 1,
    };
    writer.write_u32(idx);
}

fn read_fee_lane(reader: &mut Reader<'_>) -> binary_struct::Result<FeeLane> {
    match reader.read_u32()? {
        0 => Ok(FeeLane::Consumer),
        1 => Ok(FeeLane::Industrial),
        value => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "FeeLane",
            value,
        }),
    }
}

fn write_tx_version(writer: &mut Writer, version: TxVersion) {
    let idx = match version {
        TxVersion::Ed25519Only => 0,
        TxVersion::Dual => 1,
        TxVersion::DilithiumOnly => 2,
    };
    writer.write_u32(idx);
}

fn read_tx_version(reader: &mut Reader<'_>) -> binary_struct::Result<TxVersion> {
    match reader.read_u32()? {
        0 => Ok(TxVersion::Ed25519Only),
        1 => Ok(TxVersion::Dual),
        2 => Ok(TxVersion::DilithiumOnly),
        value => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "TxVersion",
            value,
        }),
    }
}

pub(crate) fn write_blob_tx(writer: &mut Writer, tx: &BlobTx) -> EncodeResult<()> {
    writer.write_struct(|struct_writer| {
        struct_writer.field_string("owner", &tx.owner);
        struct_writer.field_with("blob_id", |field_writer| {
            write_fixed_array(field_writer, &tx.blob_id);
        });
        struct_writer.field_with("blob_root", |field_writer| {
            write_fixed_array(field_writer, &tx.blob_root);
        });
        struct_writer.field_u64("blob_size", tx.blob_size);
        struct_writer.field_u8("fractal_lvl", tx.fractal_lvl);
        struct_writer.field_with("expiry", |field_writer| {
            field_writer
                .write_option_with(tx.expiry.as_ref(), |writer, value| writer.write_u64(*value));
        });
    });
    Ok(())
}

pub(crate) fn read_blob_tx(reader: &mut Reader<'_>) -> binary_struct::Result<BlobTx> {
    let mut owner = None;
    let mut blob_id = None;
    let mut blob_root = None;
    let mut blob_size = None;
    let mut fractal_lvl = None;
    let mut expiry = None;

    decode_struct(reader, Some(6), |key, reader| match key {
        "owner" => assign_once(&mut owner, reader.read_string()?, "owner"),
        "blob_id" => assign_once(&mut blob_id, read_fixed_array(reader)?, "blob_id"),
        "blob_root" => assign_once(&mut blob_root, read_fixed_array(reader)?, "blob_root"),
        "blob_size" => assign_once(&mut blob_size, reader.read_u64()?, "blob_size"),
        "fractal_lvl" => assign_once(&mut fractal_lvl, reader.read_u8()?, "fractal_lvl"),
        "expiry" => assign_once(
            &mut expiry,
            reader.read_option_with(|reader| reader.read_u64())?,
            "expiry",
        ),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(BlobTx {
        owner: owner.ok_or(DecodeError::MissingField("owner"))?,
        blob_id: blob_id.ok_or(DecodeError::MissingField("blob_id"))?,
        blob_root: blob_root.ok_or(DecodeError::MissingField("blob_root"))?,
        blob_size: blob_size.ok_or(DecodeError::MissingField("blob_size"))?,
        fractal_lvl: fractal_lvl.ok_or(DecodeError::MissingField("fractal_lvl"))?,
        expiry: expiry.unwrap_or(None),
    })
}

fn write_vec<T, F>(
    writer: &mut Writer,
    values: &[T],
    field: &'static str,
    mut write: F,
) -> EncodeResult<()>
where
    F: FnMut(&mut Writer, &T) -> EncodeResult<()>,
{
    let len = u64::try_from(values.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_u64(len);
    for value in values {
        write(writer, value)?;
    }
    Ok(())
}

fn read_vec<T, F>(
    reader: &mut Reader<'_>,
    _field: &'static str,
    mut read: F,
) -> Result<Vec<T>, DecodeError>
where
    F: FnMut(&mut Reader<'_>) -> Result<T, DecodeError>,
{
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(read(reader)?);
    }
    Ok(values)
}

fn write_bytes(writer: &mut Writer, value: &[u8], field: &'static str) -> EncodeResult<()> {
    let _ = u64::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_bytes(value);
    Ok(())
}

fn write_fixed_array(writer: &mut Writer, value: &[u8; 32]) {
    writer.write_u64(32);
    for byte in value {
        writer.write_u8(*byte);
    }
}

fn read_fixed_array(reader: &mut Reader<'_>) -> Result<[u8; 32], DecodeError> {
    let len = reader.read_u64()?;
    if len != 32 {
        return Err(DecodeError::InvalidFieldValue {
            field: "fixed_array",
            reason: format!("expected length 32 got {len}"),
        });
    }
    let bytes = reader.read_exact(32)?;
    let mut array = [0u8; 32];
    array.copy_from_slice(bytes);
    Ok(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> RawTxPayload {
        RawTxPayload {
            from_: "alice".into(),
            to: "bob".into(),
            amount_consumer: 10,
            amount_industrial: 5,
            fee: 2,
            pct: 75,
            nonce: 9,
            memo: vec![1, 2, 3],
        }
    }

    fn sample_signature() -> TxSignature {
        TxSignature {
            ed25519: vec![7, 8, 9],
            #[cfg(feature = "quantum")]
            dilithium: vec![4, 5, 6],
        }
    }

    fn sample_signed_tx() -> SignedTransaction {
        SignedTransaction {
            payload: sample_payload(),
            public_key: vec![11, 12, 13],
            #[cfg(feature = "quantum")]
            dilithium_public_key: vec![21, 22, 23],
            signature: sample_signature(),
            tip: 3,
            signer_pubkeys: vec![vec![1, 1], vec![2, 2, 2]],
            aggregate_signature: vec![9, 9],
            threshold: 2,
            lane: FeeLane::Industrial,
            version: TxVersion::Dual,
        }
    }

    fn sample_blob_tx() -> BlobTx {
        BlobTx {
            owner: "owner".into(),
            blob_id: [1u8; 32],
            blob_root: [2u8; 32],
            blob_size: 2048,
            fractal_lvl: 2,
            expiry: Some(77),
        }
    }

    #[test]
    fn raw_payload_round_trip() {
        let payload = sample_payload();
        let encoded = encode_raw_payload(&payload).expect("encode payload");
        let decoded = decode_raw_payload(&encoded).expect("decode payload");
        assert_eq!(decoded.from_, payload.from_);
        assert_eq!(decoded.to, payload.to);
        assert_eq!(decoded.amount_consumer, payload.amount_consumer);
        assert_eq!(decoded.amount_industrial, payload.amount_industrial);
        assert_eq!(decoded.fee, payload.fee);
        assert_eq!(decoded.pct, payload.pct);
        assert_eq!(decoded.nonce, payload.nonce);
        assert_eq!(decoded.memo, payload.memo);
    }

    #[test]
    fn signed_tx_round_trip() {
        let tx = sample_signed_tx();
        let encoded = encode_signed_transaction(&tx).expect("encode tx");
        let decoded = decode_signed_transaction(&encoded).expect("decode tx");
        assert_eq!(decoded.payload.from_, tx.payload.from_);
        assert_eq!(decoded.public_key, tx.public_key);
        #[cfg(feature = "quantum")]
        {
            assert_eq!(decoded.dilithium_public_key, tx.dilithium_public_key);
        }
        assert_eq!(decoded.signature.ed25519, tx.signature.ed25519);
        #[cfg(feature = "quantum")]
        {
            assert_eq!(decoded.signature.dilithium, tx.signature.dilithium);
        }
        assert_eq!(decoded.tip, tx.tip);
        assert_eq!(decoded.signer_pubkeys, tx.signer_pubkeys);
        assert_eq!(decoded.aggregate_signature, tx.aggregate_signature);
        assert_eq!(decoded.threshold, tx.threshold);
        assert_eq!(decoded.lane, tx.lane);
        assert_eq!(decoded.version, tx.version);
    }

    #[test]
    fn blob_tx_round_trip() {
        let tx = sample_blob_tx();
        let encoded = encode_blob_tx(&tx).expect("encode blob");
        let decoded = decode_blob_tx(&encoded).expect("decode blob");
        assert_eq!(decoded.owner, tx.owner);
        assert_eq!(decoded.blob_id, tx.blob_id);
        assert_eq!(decoded.blob_root, tx.blob_root);
        assert_eq!(decoded.blob_size, tx.blob_size);
        assert_eq!(decoded.fractal_lvl, tx.fractal_lvl);
        assert_eq!(decoded.expiry, tx.expiry);
    }
}

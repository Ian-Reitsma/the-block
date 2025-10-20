use std::convert::TryFrom;

use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::transaction::binary as tx_binary;
use crate::transaction::binary::{EncodeError, EncodeResult};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};
use crate::{Block, SignedTransaction, TokenAmount};

/// Encode a [`Block`] into the canonical binary layout.
pub fn encode_block(block: &Block) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(1024);
    write_block(&mut writer, block)?;
    Ok(writer.finish())
}

/// Decode a [`Block`] produced by [`encode_block`].
pub fn decode_block(bytes: &[u8]) -> binary_struct::Result<Block> {
    let mut reader = Reader::new(bytes);
    let block = read_block(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(block)
}

pub(crate) fn write_block(writer: &mut Writer, block: &Block) -> EncodeResult<()> {
    #[cfg(feature = "quantum")]
    const FIELD_COUNT: u64 = 28;
    #[cfg(not(feature = "quantum"))]
    const FIELD_COUNT: u64 = 26;

    writer.write_u64(FIELD_COUNT);
    writer.write_string("index");
    writer.write_u64(block.index);
    writer.write_string("previous_hash");
    writer.write_string(&block.previous_hash);
    writer.write_string("timestamp_millis");
    writer.write_u64(block.timestamp_millis);
    writer.write_string("transactions");
    write_transactions(writer, &block.transactions)?;
    writer.write_string("difficulty");
    writer.write_u64(block.difficulty);
    writer.write_string("retune_hint");
    writer.write_i64(i64::from(block.retune_hint));
    writer.write_string("nonce");
    writer.write_u64(block.nonce);
    writer.write_string("hash");
    writer.write_string(&block.hash);
    writer.write_string("coinbase_consumer");
    writer.write_u64(block.coinbase_consumer.get());
    writer.write_string("coinbase_industrial");
    writer.write_u64(block.coinbase_industrial.get());
    writer.write_string("storage_sub_ct");
    writer.write_u64(block.storage_sub_ct.get());
    writer.write_string("read_sub_ct");
    writer.write_u64(block.read_sub_ct.get());
    writer.write_string("compute_sub_ct");
    writer.write_u64(block.compute_sub_ct.get());
    writer.write_string("proof_rebate_ct");
    writer.write_u64(block.proof_rebate_ct.get());
    writer.write_string("storage_sub_it");
    writer.write_u64(block.storage_sub_it.get());
    writer.write_string("read_sub_it");
    writer.write_u64(block.read_sub_it.get());
    writer.write_string("compute_sub_it");
    writer.write_u64(block.compute_sub_it.get());
    writer.write_string("read_root");
    write_fixed(writer, &block.read_root);
    writer.write_string("fee_checksum");
    writer.write_string(&block.fee_checksum);
    writer.write_string("state_root");
    writer.write_string(&block.state_root);
    writer.write_string("base_fee");
    writer.write_u64(block.base_fee);
    writer.write_string("l2_roots");
    write_root_vec(writer, &block.l2_roots)?;
    writer.write_string("l2_sizes");
    write_u32_vec(writer, &block.l2_sizes)?;
    writer.write_string("vdf_commit");
    write_fixed(writer, &block.vdf_commit);
    writer.write_string("vdf_output");
    write_fixed(writer, &block.vdf_output);
    writer.write_string("vdf_proof");
    write_bytes(writer, &block.vdf_proof, "vdf_proof")?;
    #[cfg(feature = "quantum")]
    {
        writer.write_string("dilithium_pubkey");
        write_bytes(writer, &block.dilithium_pubkey, "dilithium_pubkey")?;
        writer.write_string("dilithium_sig");
        write_bytes(writer, &block.dilithium_sig, "dilithium_sig")?;
    }
    Ok(())
}

pub(crate) fn read_block(reader: &mut Reader<'_>) -> binary_struct::Result<Block> {
    let mut index = None;
    let mut previous_hash = None;
    let mut timestamp_millis = None;
    let mut transactions: Option<Vec<SignedTransaction>> = None;
    let mut difficulty = None;
    let mut retune_hint = None;
    let mut nonce = None;
    let mut hash = None;
    let mut coinbase_consumer = None;
    let mut coinbase_industrial = None;
    let mut storage_sub_ct = None;
    let mut read_sub_ct = None;
    let mut compute_sub_ct = None;
    let mut proof_rebate_ct = None;
    let mut storage_sub_it = None;
    let mut read_sub_it = None;
    let mut compute_sub_it = None;
    let mut read_root = None;
    let mut fee_checksum = None;
    let mut state_root = None;
    let mut base_fee = None;
    let mut l2_roots = None;
    let mut l2_sizes = None;
    let mut vdf_commit = None;
    let mut vdf_output = None;
    let mut vdf_proof = None;
    #[cfg(feature = "quantum")]
    let mut dilithium_pubkey = None;
    #[cfg(feature = "quantum")]
    let mut dilithium_sig = None;

    decode_struct(reader, None, |key, reader| match key {
        "index" => assign_once(&mut index, reader.read_u64()?, "index"),
        "previous_hash" => assign_once(&mut previous_hash, reader.read_string()?, "previous_hash"),
        "timestamp_millis" => assign_once(
            &mut timestamp_millis,
            reader.read_u64()?,
            "timestamp_millis",
        ),
        "transactions" => assign_once(
            &mut transactions,
            read_transactions(reader)?,
            "transactions",
        ),
        "difficulty" => assign_once(&mut difficulty, reader.read_u64()?, "difficulty"),
        "retune_hint" => assign_once(&mut retune_hint, read_retune_hint(reader)?, "retune_hint"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "hash" => assign_once(&mut hash, reader.read_string()?, "hash"),
        "coinbase_consumer" => assign_once(
            &mut coinbase_consumer,
            reader.read_u64()?,
            "coinbase_consumer",
        ),
        "coinbase_industrial" => assign_once(
            &mut coinbase_industrial,
            reader.read_u64()?,
            "coinbase_industrial",
        ),
        "storage_sub_ct" => assign_once(&mut storage_sub_ct, reader.read_u64()?, "storage_sub_ct"),
        "read_sub_ct" => assign_once(&mut read_sub_ct, reader.read_u64()?, "read_sub_ct"),
        "compute_sub_ct" => assign_once(&mut compute_sub_ct, reader.read_u64()?, "compute_sub_ct"),
        "proof_rebate_ct" => {
            assign_once(&mut proof_rebate_ct, reader.read_u64()?, "proof_rebate_ct")
        }
        "storage_sub_it" => assign_once(&mut storage_sub_it, reader.read_u64()?, "storage_sub_it"),
        "read_sub_it" => assign_once(&mut read_sub_it, reader.read_u64()?, "read_sub_it"),
        "compute_sub_it" => assign_once(&mut compute_sub_it, reader.read_u64()?, "compute_sub_it"),
        "read_root" => assign_once(&mut read_root, read_fixed(reader)?, "read_root"),
        "fee_checksum" => assign_once(&mut fee_checksum, reader.read_string()?, "fee_checksum"),
        "state_root" => assign_once(&mut state_root, reader.read_string()?, "state_root"),
        "base_fee" => assign_once(&mut base_fee, reader.read_u64()?, "base_fee"),
        "l2_roots" => assign_once(&mut l2_roots, read_root_vec(reader)?, "l2_roots"),
        "l2_sizes" => assign_once(&mut l2_sizes, read_u32_vec(reader)?, "l2_sizes"),
        "vdf_commit" => assign_once(&mut vdf_commit, read_fixed(reader)?, "vdf_commit"),
        "vdf_output" => assign_once(&mut vdf_output, read_fixed(reader)?, "vdf_output"),
        "vdf_proof" => assign_once(&mut vdf_proof, reader.read_bytes()?, "vdf_proof"),
        #[cfg(feature = "quantum")]
        "dilithium_pubkey" => assign_once(
            &mut dilithium_pubkey,
            reader.read_bytes()?,
            "dilithium_pubkey",
        ),
        #[cfg(feature = "quantum")]
        "dilithium_sig" => assign_once(&mut dilithium_sig, reader.read_bytes()?, "dilithium_sig"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(Block {
        index: index.ok_or(DecodeError::MissingField("index"))?,
        previous_hash: previous_hash.ok_or(DecodeError::MissingField("previous_hash"))?,
        timestamp_millis: timestamp_millis.ok_or(DecodeError::MissingField("timestamp_millis"))?,
        transactions: transactions.unwrap_or_default(),
        difficulty: difficulty.unwrap_or_default(),
        retune_hint: retune_hint.unwrap_or_default(),
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        hash: hash.ok_or(DecodeError::MissingField("hash"))?,
        coinbase_consumer: TokenAmount::new(coinbase_consumer.unwrap_or_default()),
        coinbase_industrial: TokenAmount::new(coinbase_industrial.unwrap_or_default()),
        storage_sub_ct: TokenAmount::new(storage_sub_ct.unwrap_or_default()),
        read_sub_ct: TokenAmount::new(read_sub_ct.unwrap_or_default()),
        compute_sub_ct: TokenAmount::new(compute_sub_ct.unwrap_or_default()),
        proof_rebate_ct: TokenAmount::new(proof_rebate_ct.unwrap_or_default()),
        storage_sub_it: TokenAmount::new(storage_sub_it.unwrap_or_default()),
        read_sub_it: TokenAmount::new(read_sub_it.unwrap_or_default()),
        compute_sub_it: TokenAmount::new(compute_sub_it.unwrap_or_default()),
        read_root: read_root.unwrap_or([0; 32]),
        fee_checksum: fee_checksum.unwrap_or_default(),
        state_root: state_root.unwrap_or_default(),
        base_fee: base_fee.unwrap_or_default(),
        l2_roots: l2_roots.unwrap_or_default(),
        l2_sizes: l2_sizes.unwrap_or_default(),
        vdf_commit: vdf_commit.unwrap_or([0; 32]),
        vdf_output: vdf_output.unwrap_or([0; 32]),
        vdf_proof: vdf_proof.unwrap_or_default(),
        #[cfg(feature = "quantum")]
        dilithium_pubkey: dilithium_pubkey.unwrap_or_default(),
        #[cfg(feature = "quantum")]
        dilithium_sig: dilithium_sig.unwrap_or_default(),
    })
}

fn write_transactions(writer: &mut Writer, txs: &[SignedTransaction]) -> EncodeResult<()> {
    write_vec(writer, txs, "transactions", |writer, tx| {
        tx_binary::write_signed_transaction(writer, tx)
    })
}

fn read_transactions(reader: &mut Reader<'_>) -> Result<Vec<SignedTransaction>, DecodeError> {
    read_vec(reader, |reader| tx_binary::read_signed_transaction(reader))
}

fn write_root_vec(writer: &mut Writer, roots: &[[u8; 32]]) -> EncodeResult<()> {
    write_vec(writer, roots, "l2_roots", |writer, root| {
        write_fixed(writer, root);
        Ok(())
    })
}

fn read_root_vec(reader: &mut Reader<'_>) -> Result<Vec<[u8; 32]>, DecodeError> {
    read_vec(reader, |reader| read_fixed(reader))
}

fn write_u32_vec(writer: &mut Writer, values: &[u32]) -> EncodeResult<()> {
    write_vec(writer, values, "l2_sizes", |writer, value| {
        writer.write_u32(*value);
        Ok(())
    })
}

fn read_u32_vec(reader: &mut Reader<'_>) -> Result<Vec<u32>, DecodeError> {
    read_vec(reader, |reader| {
        reader.read_u32().map_err(DecodeError::from)
    })
}

fn write_bytes(writer: &mut Writer, value: &[u8], field: &'static str) -> EncodeResult<()> {
    let _ = u64::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_bytes(value);
    Ok(())
}

fn write_fixed(writer: &mut Writer, value: &[u8; 32]) {
    writer.write_u64(32);
    for byte in value {
        writer.write_u8(*byte);
    }
}

fn read_fixed(reader: &mut Reader<'_>) -> Result<[u8; 32], DecodeError> {
    let len = reader.read_u64()?;
    if len != 32 {
        return Err(DecodeError::InvalidFieldValue {
            field: "fixed_array",
            reason: format!("expected length 32 got {len}"),
        });
    }
    let bytes = reader.read_exact(32)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes);
    Ok(out)
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

fn read_vec<T, F>(reader: &mut Reader<'_>, mut read: F) -> Result<Vec<T>, DecodeError>
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

fn read_retune_hint(reader: &mut Reader<'_>) -> Result<i8, DecodeError> {
    let value = reader.read_i64()?;
    i8::try_from(value).map_err(|_| DecodeError::InvalidFieldValue {
        field: "retune_hint",
        reason: format!("expected i8 got {value}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{FeeLane, RawTxPayload, SignedTransaction, TxSignature, TxVersion};

    fn sample_tx() -> SignedTransaction {
        SignedTransaction {
            payload: RawTxPayload {
                from_: "alice".into(),
                to: "bob".into(),
                amount_consumer: 1,
                amount_industrial: 2,
                fee: 3,
                pct_ct: 50,
                nonce: 7,
                memo: vec![1, 2, 3],
            },
            public_key: vec![1, 2, 3, 4],
            #[cfg(feature = "quantum")]
            dilithium_public_key: vec![9, 9],
            signature: TxSignature {
                ed25519: vec![5, 6],
                #[cfg(feature = "quantum")]
                dilithium: vec![7, 8],
            },
            tip: 11,
            signer_pubkeys: vec![vec![1], vec![2, 2]],
            aggregate_signature: vec![0xaa, 0xbb],
            threshold: 1,
            lane: FeeLane::Consumer,
            version: TxVersion::Ed25519Only,
        }
    }

    fn sample_block() -> Block {
        Block {
            index: 4,
            previous_hash: "prev".into(),
            timestamp_millis: 99,
            transactions: vec![sample_tx()],
            difficulty: 5,
            retune_hint: -2,
            nonce: 42,
            hash: "hash".into(),
            coinbase_consumer: TokenAmount::new(10),
            coinbase_industrial: TokenAmount::new(11),
            storage_sub_ct: TokenAmount::new(12),
            read_sub_ct: TokenAmount::new(13),
            compute_sub_ct: TokenAmount::new(14),
            proof_rebate_ct: TokenAmount::new(15),
            storage_sub_it: TokenAmount::new(16),
            read_sub_it: TokenAmount::new(17),
            compute_sub_it: TokenAmount::new(18),
            read_root: [1u8; 32],
            fee_checksum: "fee".into(),
            state_root: "state".into(),
            base_fee: 7,
            l2_roots: vec![[2u8; 32], [3u8; 32]],
            l2_sizes: vec![4, 5],
            vdf_commit: [4u8; 32],
            vdf_output: [5u8; 32],
            vdf_proof: vec![1, 2, 3],
            #[cfg(feature = "quantum")]
            dilithium_pubkey: vec![1, 3, 5],
            #[cfg(feature = "quantum")]
            dilithium_sig: vec![2, 4, 6],
        }
    }

    #[test]
    fn block_round_trip() {
        let block = sample_block();
        let encoded = encode_block(&block).expect("encode block");
        let decoded = decode_block(&encoded).expect("decode block");
        assert_eq!(decoded.index, block.index);
        assert_eq!(decoded.previous_hash, block.previous_hash);
        assert_eq!(decoded.timestamp_millis, block.timestamp_millis);
        assert_eq!(decoded.transactions.len(), block.transactions.len());
        assert_eq!(decoded.difficulty, block.difficulty);
        assert_eq!(decoded.retune_hint, block.retune_hint);
        assert_eq!(decoded.nonce, block.nonce);
        assert_eq!(decoded.hash, block.hash);
        assert_eq!(
            decoded.coinbase_consumer.get(),
            block.coinbase_consumer.get()
        );
        assert_eq!(
            decoded.coinbase_industrial.get(),
            block.coinbase_industrial.get()
        );
        assert_eq!(decoded.storage_sub_ct.get(), block.storage_sub_ct.get());
        assert_eq!(decoded.read_sub_ct.get(), block.read_sub_ct.get());
        assert_eq!(decoded.compute_sub_ct.get(), block.compute_sub_ct.get());
        assert_eq!(decoded.proof_rebate_ct.get(), block.proof_rebate_ct.get());
        assert_eq!(decoded.storage_sub_it.get(), block.storage_sub_it.get());
        assert_eq!(decoded.read_sub_it.get(), block.read_sub_it.get());
        assert_eq!(decoded.compute_sub_it.get(), block.compute_sub_it.get());
        assert_eq!(decoded.read_root, block.read_root);
        assert_eq!(decoded.fee_checksum, block.fee_checksum);
        assert_eq!(decoded.state_root, block.state_root);
        assert_eq!(decoded.base_fee, block.base_fee);
        assert_eq!(decoded.l2_roots, block.l2_roots);
        assert_eq!(decoded.l2_sizes, block.l2_sizes);
        assert_eq!(decoded.vdf_commit, block.vdf_commit);
        assert_eq!(decoded.vdf_output, block.vdf_output);
        assert_eq!(decoded.vdf_proof, block.vdf_proof);
        #[cfg(feature = "quantum")]
        {
            assert_eq!(decoded.dilithium_pubkey, block.dilithium_pubkey);
            assert_eq!(decoded.dilithium_sig, block.dilithium_sig);
        }
    }
}

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
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_u64("index", block.index);
        struct_writer.field_string("previous_hash", &block.previous_hash);
        struct_writer.field_u64("timestamp_millis", block.timestamp_millis);
        struct_writer.field_with("transactions", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_transactions(field_writer, &block.transactions) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_u64("difficulty", block.difficulty);
        struct_writer.field_i64("retune_hint", i64::from(block.retune_hint));
        struct_writer.field_u64("nonce", block.nonce);
        struct_writer.field_string("hash", &block.hash);
        struct_writer.field_u64("coinbase_consumer", block.coinbase_consumer.get());
        struct_writer.field_u64("coinbase_industrial", block.coinbase_industrial.get());
        struct_writer.field_u64("storage_sub_ct", block.storage_sub_ct.get());
        struct_writer.field_u64("read_sub_ct", block.read_sub_ct.get());
        struct_writer.field_u64("read_sub_viewer_ct", block.read_sub_viewer_ct.get());
        struct_writer.field_u64("read_sub_host_ct", block.read_sub_host_ct.get());
        struct_writer.field_u64("read_sub_hardware_ct", block.read_sub_hardware_ct.get());
        struct_writer.field_u64("read_sub_verifier_ct", block.read_sub_verifier_ct.get());
        struct_writer.field_u64("read_sub_liquidity_ct", block.read_sub_liquidity_ct.get());
        struct_writer.field_u64("ad_viewer_ct", block.ad_viewer_ct.get());
        struct_writer.field_u64("ad_host_ct", block.ad_host_ct.get());
        struct_writer.field_u64("ad_hardware_ct", block.ad_hardware_ct.get());
        struct_writer.field_u64("ad_verifier_ct", block.ad_verifier_ct.get());
        struct_writer.field_u64("ad_liquidity_ct", block.ad_liquidity_ct.get());
        struct_writer.field_u64("ad_miner_ct", block.ad_miner_ct.get());
        struct_writer.field_u64("ad_host_it", block.ad_host_it.get());
        struct_writer.field_u64("ad_hardware_it", block.ad_hardware_it.get());
        struct_writer.field_u64("ad_verifier_it", block.ad_verifier_it.get());
        struct_writer.field_u64("ad_liquidity_it", block.ad_liquidity_it.get());
        struct_writer.field_u64("ad_miner_it", block.ad_miner_it.get());
        struct_writer.field_u64("ad_total_usd_micros", block.ad_total_usd_micros);
        struct_writer.field_u64("ad_settlement_count", block.ad_settlement_count);
        struct_writer.field_u64(
            "ad_oracle_ct_price_usd_micros",
            block.ad_oracle_ct_price_usd_micros,
        );
        struct_writer.field_u64(
            "ad_oracle_it_price_usd_micros",
            block.ad_oracle_it_price_usd_micros,
        );
        struct_writer.field_u64("compute_sub_ct", block.compute_sub_ct.get());
        struct_writer.field_u64("proof_rebate_ct", block.proof_rebate_ct.get());
        struct_writer.field_u64("storage_sub_it", block.storage_sub_it.get());
        struct_writer.field_u64("read_sub_it", block.read_sub_it.get());
        struct_writer.field_u64("compute_sub_it", block.compute_sub_it.get());
        struct_writer.field_with("read_root", |field_writer| {
            write_fixed(field_writer, &block.read_root);
        });
        struct_writer.field_string("fee_checksum", &block.fee_checksum);
        struct_writer.field_string("state_root", &block.state_root);
        struct_writer.field_u64("base_fee", block.base_fee);
        struct_writer.field_with("l2_roots", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_root_vec(field_writer, &block.l2_roots) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("l2_sizes", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_u32_vec(field_writer, &block.l2_sizes) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("vdf_commit", |field_writer| {
            write_fixed(field_writer, &block.vdf_commit);
        });
        struct_writer.field_with("vdf_output", |field_writer| {
            write_fixed(field_writer, &block.vdf_output);
        });
        struct_writer.field_with("vdf_proof", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_bytes(field_writer, &block.vdf_proof, "vdf_proof") {
                    result = Err(err);
                }
            }
        });
        #[cfg(feature = "quantum")]
        {
            struct_writer.field_with("dilithium_pubkey", |field_writer| {
                if result.is_ok() {
                    if let Err(err) =
                        write_bytes(field_writer, &block.dilithium_pubkey, "dilithium_pubkey")
                    {
                        result = Err(err);
                    }
                }
            });
            struct_writer.field_with("dilithium_sig", |field_writer| {
                if result.is_ok() {
                    if let Err(err) =
                        write_bytes(field_writer, &block.dilithium_sig, "dilithium_sig")
                    {
                        result = Err(err);
                    }
                }
            });
        }
    });
    result
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
    let mut read_sub_viewer_ct = None;
    let mut read_sub_host_ct = None;
    let mut read_sub_hardware_ct = None;
    let mut read_sub_verifier_ct = None;
    let mut read_sub_liquidity_ct = None;
    let mut ad_viewer_ct = None;
    let mut ad_host_ct = None;
    let mut ad_hardware_ct = None;
    let mut ad_verifier_ct = None;
    let mut ad_liquidity_ct = None;
    let mut ad_miner_ct = None;
    let mut ad_host_it = None;
    let mut ad_hardware_it = None;
    let mut ad_verifier_it = None;
    let mut ad_liquidity_it = None;
    let mut ad_miner_it = None;
    let mut ad_total_usd_micros = None;
    let mut ad_settlement_count = None;
    let mut ad_oracle_ct_price_usd_micros = None;
    let mut ad_oracle_it_price_usd_micros = None;
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
        "read_sub_viewer_ct" => assign_once(
            &mut read_sub_viewer_ct,
            reader.read_u64()?,
            "read_sub_viewer_ct",
        ),
        "read_sub_host_ct" => assign_once(
            &mut read_sub_host_ct,
            reader.read_u64()?,
            "read_sub_host_ct",
        ),
        "read_sub_hardware_ct" => assign_once(
            &mut read_sub_hardware_ct,
            reader.read_u64()?,
            "read_sub_hardware_ct",
        ),
        "read_sub_verifier_ct" => assign_once(
            &mut read_sub_verifier_ct,
            reader.read_u64()?,
            "read_sub_verifier_ct",
        ),
        "read_sub_liquidity_ct" => assign_once(
            &mut read_sub_liquidity_ct,
            reader.read_u64()?,
            "read_sub_liquidity_ct",
        ),
        "ad_viewer_ct" => assign_once(&mut ad_viewer_ct, reader.read_u64()?, "ad_viewer_ct"),
        "ad_host_ct" => assign_once(&mut ad_host_ct, reader.read_u64()?, "ad_host_ct"),
        "ad_hardware_ct" => assign_once(&mut ad_hardware_ct, reader.read_u64()?, "ad_hardware_ct"),
        "ad_verifier_ct" => assign_once(&mut ad_verifier_ct, reader.read_u64()?, "ad_verifier_ct"),
        "ad_liquidity_ct" => {
            assign_once(&mut ad_liquidity_ct, reader.read_u64()?, "ad_liquidity_ct")
        }
        "ad_miner_ct" => assign_once(&mut ad_miner_ct, reader.read_u64()?, "ad_miner_ct"),
        "ad_host_it" => assign_once(&mut ad_host_it, reader.read_u64()?, "ad_host_it"),
        "ad_hardware_it" => assign_once(&mut ad_hardware_it, reader.read_u64()?, "ad_hardware_it"),
        "ad_verifier_it" => assign_once(&mut ad_verifier_it, reader.read_u64()?, "ad_verifier_it"),
        "ad_liquidity_it" => {
            assign_once(&mut ad_liquidity_it, reader.read_u64()?, "ad_liquidity_it")
        }
        "ad_miner_it" => assign_once(&mut ad_miner_it, reader.read_u64()?, "ad_miner_it"),
        "ad_total_usd_micros" => assign_once(
            &mut ad_total_usd_micros,
            reader.read_u64()?,
            "ad_total_usd_micros",
        ),
        "ad_settlement_count" => assign_once(
            &mut ad_settlement_count,
            reader.read_u64()?,
            "ad_settlement_count",
        ),
        "ad_oracle_ct_price_usd_micros" => assign_once(
            &mut ad_oracle_ct_price_usd_micros,
            reader.read_u64()?,
            "ad_oracle_ct_price_usd_micros",
        ),
        "ad_oracle_it_price_usd_micros" => assign_once(
            &mut ad_oracle_it_price_usd_micros,
            reader.read_u64()?,
            "ad_oracle_it_price_usd_micros",
        ),
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
        read_sub_viewer_ct: TokenAmount::new(read_sub_viewer_ct.unwrap_or_default()),
        read_sub_host_ct: TokenAmount::new(read_sub_host_ct.unwrap_or_default()),
        read_sub_hardware_ct: TokenAmount::new(read_sub_hardware_ct.unwrap_or_default()),
        read_sub_verifier_ct: TokenAmount::new(read_sub_verifier_ct.unwrap_or_default()),
        read_sub_liquidity_ct: TokenAmount::new(read_sub_liquidity_ct.unwrap_or_default()),
        ad_viewer_ct: TokenAmount::new(ad_viewer_ct.unwrap_or_default()),
        ad_host_ct: TokenAmount::new(ad_host_ct.unwrap_or_default()),
        ad_hardware_ct: TokenAmount::new(ad_hardware_ct.unwrap_or_default()),
        ad_verifier_ct: TokenAmount::new(ad_verifier_ct.unwrap_or_default()),
        ad_liquidity_ct: TokenAmount::new(ad_liquidity_ct.unwrap_or_default()),
        ad_miner_ct: TokenAmount::new(ad_miner_ct.unwrap_or_default()),
        ad_host_it: TokenAmount::new(ad_host_it.unwrap_or_default()),
        ad_hardware_it: TokenAmount::new(ad_hardware_it.unwrap_or_default()),
        ad_verifier_it: TokenAmount::new(ad_verifier_it.unwrap_or_default()),
        ad_liquidity_it: TokenAmount::new(ad_liquidity_it.unwrap_or_default()),
        ad_miner_it: TokenAmount::new(ad_miner_it.unwrap_or_default()),
        ad_total_usd_micros: ad_total_usd_micros.unwrap_or_default(),
        ad_settlement_count: ad_settlement_count.unwrap_or_default(),
        ad_oracle_ct_price_usd_micros: ad_oracle_ct_price_usd_micros.unwrap_or_default(),
        ad_oracle_it_price_usd_micros: ad_oracle_it_price_usd_micros.unwrap_or_default(),
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
            read_sub_viewer_ct: TokenAmount::new(2),
            read_sub_host_ct: TokenAmount::new(3),
            read_sub_hardware_ct: TokenAmount::new(4),
            read_sub_verifier_ct: TokenAmount::new(1),
            read_sub_liquidity_ct: TokenAmount::new(3),
            ad_viewer_ct: TokenAmount::new(6),
            ad_host_ct: TokenAmount::new(7),
            ad_hardware_ct: TokenAmount::new(8),
            ad_verifier_ct: TokenAmount::new(9),
            ad_liquidity_ct: TokenAmount::new(10),
            ad_miner_ct: TokenAmount::new(11),
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

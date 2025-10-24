use std::convert::TryFrom;
use std::net::SocketAddr;

use crate::block_binary;
use crate::net::peer::ReputationUpdate;
use crate::p2p::handshake::Hello;
use crate::p2p::wire_binary;
use crate::transaction::binary::{self as tx_binary, EncodeError, EncodeResult};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};
use crate::{BlobTx, Block, SignedTransaction};
use concurrency::Bytes;
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::binary_cursor::{
    CursorError, Reader as BinaryReader, Writer as BinaryWriter,
};
use foundation_serialization::{Deserialize, Serialize};
use ledger::address::ShardId;

/// Signed network message wrapper.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    /// Sender public key.
    pub pubkey: [u8; 32],
    /// Signature over the encoded body.
    pub signature: Bytes,
    /// Inner message payload.
    pub body: Payload,
    /// Optional partition marker propagated in gossip headers.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub partition: Option<u64>,
    /// Optional certificate fingerprint for QUIC trust validation.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub cert_fingerprint: Option<Bytes>,
}

impl Message {
    /// Sign `body` with `kp` producing an authenticated message.
    pub fn new(body: Payload, sk: &SigningKey) -> Self {
        let bytes = encode_payload(&body).unwrap_or_else(|e| panic!("serialize message body: {e}"));
        let sig = sk.sign(&bytes);
        Self {
            pubkey: sk.verifying_key().to_bytes(),
            signature: Bytes::from(sig.to_bytes().to_vec()),
            body,
            partition: None,
            cert_fingerprint: {
                #[cfg(feature = "quic")]
                {
                    crate::net::transport_quic::current_advertisement()
                        .map(|ad| Bytes::from(ad.fingerprint.to_vec()))
                }
                #[cfg(not(feature = "quic"))]
                {
                    None
                }
            },
        }
    }
}

/// Network message payloads exchanged between peers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Payload {
    /// Version/feature negotiation and identity exchange.
    Handshake(Hello),
    /// Advertise known peers.
    Hello(Vec<SocketAddr>),
    /// Broadcast a transaction to be relayed and mined.
    Tx(SignedTransaction),
    /// Broadcast a blob transaction for inclusion in L2 blobspace.
    BlobTx(BlobTx),
    /// Broadcast a newly mined block for a given shard.
    Block(ShardId, Block),
    /// Share an entire chain snapshot for fork resolution.
    Chain(Vec<Block>),
    /// Disseminate a single erasure-coded shard of a blob.
    BlobChunk(BlobChunk),
    /// Propagate provider reputation scores.
    Reputation(Vec<ReputationUpdate>),
}

/// Individual erasure-coded shard associated with a blob root.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BlobChunk {
    /// Commitment root this shard belongs to.
    pub root: [u8; 32],
    /// Index of this shard in the erasure-coded set.
    pub index: u32,
    /// Total number of shards.
    pub total: u32,
    /// Raw shard bytes.
    pub data: Bytes,
}

// ReputationUpdate defined in peer.rs

/// Attempt to decode a [`Message`] from raw bytes.
#[allow(dead_code)]
pub fn decode(bytes: &[u8]) -> binary_struct::Result<Message> {
    let mut reader = BinaryReader::new(bytes);
    let message = read_message(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(message)
}

/// Encode a [`Message`] into the canonical binary layout.
pub fn encode_message(message: &Message) -> EncodeResult<Vec<u8>> {
    let mut writer = BinaryWriter::with_capacity(256);
    write_message(&mut writer, message)?;
    Ok(writer.finish())
}

/// Encode a [`Payload`] for signing and transport.
pub fn encode_payload(payload: &Payload) -> EncodeResult<Vec<u8>> {
    let mut writer = BinaryWriter::with_capacity(256);
    write_payload(&mut writer, payload)?;
    Ok(writer.finish())
}

fn write_message(writer: &mut BinaryWriter, message: &Message) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("pubkey", |field_writer| {
            write_fixed(field_writer, &message.pubkey);
        });
        struct_writer.field_with("signature", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_bytes(field_writer, &message.signature, "signature") {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("body", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_payload(field_writer, &message.body) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("partition", |field_writer| {
            field_writer.write_option_with(message.partition.as_ref(), |writer, value| {
                writer.write_u64(*value)
            });
        });
        struct_writer.field_with("cert_fingerprint", |field_writer| {
            field_writer.write_bool(message.cert_fingerprint.is_some());
            if let Some(value) = message.cert_fingerprint.as_ref() {
                if result.is_ok() {
                    if let Err(err) = write_bytes(field_writer, value, "cert_fingerprint") {
                        result = Err(err);
                    }
                }
            }
        });
    });
    result
}

fn read_message(reader: &mut BinaryReader<'_>) -> binary_struct::Result<Message> {
    let mut pubkey = None;
    let mut signature = None;
    let mut body = None;
    let mut partition = None;
    let mut cert_fingerprint = None;

    decode_struct(reader, Some(5), |key, reader| match key {
        "pubkey" => assign_once(&mut pubkey, read_fixed(reader)?, "pubkey"),
        "signature" => assign_once(&mut signature, read_bytes(reader)?, "signature"),
        "body" => assign_once(&mut body, read_payload(reader)?, "body"),
        "partition" => assign_once(
            &mut partition,
            reader.read_option_with(|reader| reader.read_u64())?,
            "partition",
        ),
        "cert_fingerprint" => {
            let has_value = reader.read_bool()?;
            let value = if has_value {
                Some(read_bytes(reader)?)
            } else {
                None
            };
            assign_once(&mut cert_fingerprint, value, "cert_fingerprint")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(Message {
        pubkey: pubkey.ok_or(DecodeError::MissingField("pubkey"))?,
        signature: signature.unwrap_or_else(Bytes::new),
        body: body.ok_or(DecodeError::MissingField("body"))?,
        partition: partition.unwrap_or(None),
        cert_fingerprint: cert_fingerprint.unwrap_or(None),
    })
}

fn write_payload(writer: &mut BinaryWriter, payload: &Payload) -> EncodeResult<()> {
    match payload {
        Payload::Handshake(hello) => {
            writer.write_u32(0);
            write_hello_payload(writer, hello)?;
        }
        Payload::Hello(addrs) => {
            writer.write_u32(1);
            write_socket_addrs(writer, addrs)?;
        }
        Payload::Tx(tx) => {
            writer.write_u32(2);
            tx_binary::write_signed_transaction(writer, tx)?;
        }
        Payload::BlobTx(blob) => {
            writer.write_u32(3);
            tx_binary::write_blob_tx(writer, blob)?;
        }
        Payload::Block(shard, block) => {
            writer.write_u32(4);
            writer.write_u16((*shard).into());
            block_binary::write_block(writer, block)?;
        }
        Payload::Chain(chain) => {
            writer.write_u32(5);
            write_vec(writer, chain, "chain", |writer, block| {
                block_binary::write_block(writer, block)
            })?;
        }
        Payload::BlobChunk(chunk) => {
            writer.write_u32(6);
            write_blob_chunk(writer, chunk)?;
        }
        Payload::Reputation(updates) => {
            writer.write_u32(7);
            write_vec(writer, updates, "reputation", |writer, update| {
                write_reputation_update(writer, update)
            })?;
        }
    }
    Ok(())
}

fn read_payload(reader: &mut BinaryReader<'_>) -> binary_struct::Result<Payload> {
    match reader.read_u32()? {
        0 => Ok(Payload::Handshake(wire_binary::read_hello(reader)?)),
        1 => Ok(Payload::Hello(read_socket_addrs(reader)?)),
        2 => Ok(Payload::Tx(tx_binary::read_signed_transaction(reader)?)),
        3 => Ok(Payload::BlobTx(tx_binary::read_blob_tx(reader)?)),
        4 => {
            let shard = reader.read_u16()?;
            let block = block_binary::read_block(reader)?;
            Ok(Payload::Block(shard, block))
        }
        5 => {
            let chain = read_vec(reader, |reader| block_binary::read_block(reader))?;
            Ok(Payload::Chain(chain))
        }
        6 => Ok(Payload::BlobChunk(read_blob_chunk(reader)?)),
        7 => Ok(Payload::Reputation(read_vec(
            reader,
            read_reputation_update,
        )?)),
        other => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "Payload",
            value: other,
        }),
    }
}

fn write_blob_chunk(writer: &mut BinaryWriter, chunk: &BlobChunk) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("root", |field_writer| {
            write_fixed(field_writer, &chunk.root);
        });
        struct_writer.field_u32("index", chunk.index);
        struct_writer.field_u32("total", chunk.total);
        struct_writer.field_with("data", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_bytes(field_writer, &chunk.data, "data") {
                    result = Err(err);
                }
            }
        });
    });
    result
}

fn read_blob_chunk(reader: &mut BinaryReader<'_>) -> binary_struct::Result<BlobChunk> {
    let mut root = None;
    let mut index = None;
    let mut total = None;
    let mut data = None;

    decode_struct(reader, Some(4), |key, reader| match key {
        "root" => assign_once(&mut root, read_fixed(reader)?, "root"),
        "index" => assign_once(&mut index, reader.read_u32()?, "index"),
        "total" => assign_once(&mut total, reader.read_u32()?, "total"),
        "data" => assign_once(&mut data, read_bytes(reader)?, "data"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(BlobChunk {
        root: root.ok_or(DecodeError::MissingField("root"))?,
        index: index.ok_or(DecodeError::MissingField("index"))?,
        total: total.ok_or(DecodeError::MissingField("total"))?,
        data: data.unwrap_or_else(Bytes::new),
    })
}

fn write_reputation_update(
    writer: &mut BinaryWriter,
    update: &ReputationUpdate,
) -> EncodeResult<()> {
    writer.write_struct(|struct_writer| {
        struct_writer.field_string("provider_id", &update.provider_id);
        struct_writer.field_i64("reputation_score", update.reputation_score);
        struct_writer.field_u64("epoch", update.epoch);
    });
    Ok(())
}

fn read_reputation_update(
    reader: &mut BinaryReader<'_>,
) -> binary_struct::Result<ReputationUpdate> {
    let mut provider_id = None;
    let mut reputation_score = None;
    let mut epoch = None;

    decode_struct(reader, Some(3), |key, reader| match key {
        "provider_id" => assign_once(&mut provider_id, reader.read_string()?, "provider_id"),
        "reputation_score" => assign_once(
            &mut reputation_score,
            reader.read_i64()?,
            "reputation_score",
        ),
        "epoch" => assign_once(&mut epoch, reader.read_u64()?, "epoch"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(ReputationUpdate {
        provider_id: provider_id.ok_or(DecodeError::MissingField("provider_id"))?,
        reputation_score: reputation_score.ok_or(DecodeError::MissingField("reputation_score"))?,
        epoch: epoch.ok_or(DecodeError::MissingField("epoch"))?,
    })
}

fn write_socket_addrs(writer: &mut BinaryWriter, addrs: &[SocketAddr]) -> EncodeResult<()> {
    write_vec(writer, addrs, "hello_addrs", |writer, addr| {
        writer.write_string(&addr.to_string());
        Ok(())
    })
}

fn read_socket_addrs(reader: &mut BinaryReader<'_>) -> Result<Vec<SocketAddr>, DecodeError> {
    read_vec(reader, |reader| {
        let value = reader.read_string()?;
        value.parse().map_err(
            |err: std::net::AddrParseError| DecodeError::InvalidFieldValue {
                field: "SocketAddr",
                reason: err.to_string(),
            },
        )
    })
}

fn write_bytes(writer: &mut BinaryWriter, value: &Bytes, field: &'static str) -> EncodeResult<()> {
    let _ = u64::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_bytes(value);
    Ok(())
}

fn read_bytes(reader: &mut BinaryReader<'_>) -> Result<Bytes, DecodeError> {
    let len = reader.read_u64()?;
    let len_usize =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let bytes = reader.read_exact(len_usize)?;
    Ok(Bytes::from(bytes.to_vec()))
}

fn write_fixed(writer: &mut BinaryWriter, value: &[u8; 32]) {
    writer.write_u64(32);
    for byte in value {
        writer.write_u8(*byte);
    }
}

fn read_fixed(reader: &mut BinaryReader<'_>) -> Result<[u8; 32], DecodeError> {
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
    writer: &mut BinaryWriter,
    values: &[T],
    field: &'static str,
    mut write: F,
) -> EncodeResult<()>
where
    F: FnMut(&mut BinaryWriter, &T) -> EncodeResult<()>,
{
    let len = u64::try_from(values.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_u64(len);
    for value in values {
        write(writer, value)?;
    }
    Ok(())
}

fn read_vec<T, F>(reader: &mut BinaryReader<'_>, mut read: F) -> Result<Vec<T>, DecodeError>
where
    F: FnMut(&mut BinaryReader<'_>) -> Result<T, DecodeError>,
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

fn write_hello_payload(writer: &mut BinaryWriter, hello: &Hello) -> EncodeResult<()> {
    wire_binary::write_hello(writer, hello).map_err(|err| match err {
        wire_binary::EncodeError::LengthOverflow(field) => EncodeError::LengthOverflow(field),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::handshake::{Hello, Transport};
    use crate::transaction::{FeeLane, RawTxPayload, SignedTransaction, TxSignature, TxVersion};
    use crate::util::binary_struct::ensure_exhausted;
    use crate::TokenAmount;

    fn sample_raw_tx() -> RawTxPayload {
        RawTxPayload {
            from_: "alice".into(),
            to: "bob".into(),
            amount_consumer: 10,
            amount_industrial: 20,
            fee: 2,
            pct_ct: 64,
            nonce: 9,
            memo: vec![1, 2, 3],
        }
    }

    fn sample_signed_tx() -> SignedTransaction {
        SignedTransaction {
            payload: sample_raw_tx(),
            public_key: vec![1, 2, 3],
            #[cfg(feature = "quantum")]
            dilithium_public_key: vec![4, 5, 6],
            signature: TxSignature {
                ed25519: vec![7, 8, 9],
                #[cfg(feature = "quantum")]
                dilithium: vec![10, 11, 12],
            },
            tip: 5,
            signer_pubkeys: vec![vec![1], vec![2, 2]],
            aggregate_signature: vec![0xaa, 0xbb],
            threshold: 2,
            lane: FeeLane::Industrial,
            version: TxVersion::Dual,
        }
    }

    fn sample_block() -> Block {
        Block {
            index: 1,
            previous_hash: "prev".into(),
            timestamp_millis: 1234,
            transactions: vec![sample_signed_tx()],
            difficulty: 8,
            retune_hint: -1,
            nonce: 99,
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
            ad_viewer_ct: TokenAmount::new(5),
            ad_host_ct: TokenAmount::new(6),
            ad_hardware_ct: TokenAmount::new(7),
            ad_verifier_ct: TokenAmount::new(8),
            ad_liquidity_ct: TokenAmount::new(9),
            ad_miner_ct: TokenAmount::new(10),
            compute_sub_ct: TokenAmount::new(14),
            proof_rebate_ct: TokenAmount::new(15),
            storage_sub_it: TokenAmount::new(16),
            read_sub_it: TokenAmount::new(17),
            compute_sub_it: TokenAmount::new(18),
            read_root: [1u8; 32],
            fee_checksum: "fee".into(),
            state_root: "state".into(),
            base_fee: 3,
            l2_roots: vec![[2u8; 32]],
            l2_sizes: vec![32],
            vdf_commit: [3u8; 32],
            vdf_output: [4u8; 32],
            vdf_proof: vec![9, 9],
            #[cfg(feature = "quantum")]
            dilithium_pubkey: vec![1, 3, 5],
            #[cfg(feature = "quantum")]
            dilithium_sig: vec![2, 4, 6],
        }
    }

    fn sample_blob_tx() -> BlobTx {
        BlobTx {
            owner: "owner".into(),
            blob_id: [5u8; 32],
            blob_root: [6u8; 32],
            blob_size: 2048,
            fractal_lvl: 3,
            expiry: Some(99),
        }
    }

    fn sample_reputation() -> ReputationUpdate {
        ReputationUpdate {
            provider_id: "provider-a".into(),
            reputation_score: 99,
            epoch: 7,
        }
    }

    fn sample_hello() -> Hello {
        Hello {
            network_id: [1, 2, 3, 4],
            proto_version: 1,
            feature_bits: 0b1010,
            agent: "blockd/1.0".into(),
            nonce: 42,
            transport: Transport::Tcp,
            quic_addr: Some("127.0.0.1:9000".parse().unwrap()),
            quic_cert: Some(Bytes::from(vec![7, 7, 7])),
            quic_fingerprint: Some(Bytes::from(vec![1, 1])),
            quic_fingerprint_previous: vec![Bytes::from(vec![2, 2])],
            quic_provider: Some("provider".into()),
            quic_capabilities: vec!["rotation".into()],
        }
    }

    fn round_trip_payload(payload: Payload) -> Payload {
        let mut writer = BinaryWriter::with_capacity(256);
        write_payload(&mut writer, &payload).expect("encode payload");
        let bytes = writer.finish();
        let mut reader = BinaryReader::new(&bytes);
        let decoded = read_payload(&mut reader).expect("decode payload");
        ensure_exhausted(&reader).expect("payload exhausted");
        let mut writer_again = BinaryWriter::with_capacity(256);
        write_payload(&mut writer_again, &decoded).expect("re-encode payload");
        assert_eq!(writer_again.finish(), bytes);
        decoded
    }

    #[test]
    fn handshake_payload_round_trips() {
        let hello = sample_hello();
        let decoded = round_trip_payload(Payload::Handshake(hello.clone()));
        match decoded {
            Payload::Handshake(actual) => assert_eq!(actual, hello),
            other => panic!("expected handshake, got {other:?}"),
        }
    }

    #[test]
    fn hello_payload_round_trips() {
        let addrs = vec![
            "127.0.0.1:7000".parse().unwrap(),
            "10.0.0.5:9000".parse().unwrap(),
        ];
        let decoded = round_trip_payload(Payload::Hello(addrs.clone()));
        match decoded {
            Payload::Hello(actual) => assert_eq!(actual, addrs),
            other => panic!("expected hello, got {other:?}"),
        }
    }

    #[test]
    fn signed_tx_payload_round_trips() {
        let expected = sample_signed_tx();
        let decoded = round_trip_payload(Payload::Tx(sample_signed_tx()));
        match decoded {
            Payload::Tx(actual) => {
                assert_eq!(actual.payload.from_, expected.payload.from_);
                assert_eq!(actual.public_key, expected.public_key);
                assert_eq!(actual.signature.ed25519, expected.signature.ed25519);
                #[cfg(feature = "quantum")]
                {
                    assert_eq!(actual.signature.dilithium, expected.signature.dilithium);
                }
                assert_eq!(actual.tip, expected.tip);
                assert_eq!(actual.signer_pubkeys, expected.signer_pubkeys);
                assert_eq!(actual.aggregate_signature, expected.aggregate_signature);
                assert_eq!(actual.threshold, expected.threshold);
                assert_eq!(actual.lane, expected.lane);
                assert_eq!(actual.version, expected.version);
            }
            other => panic!("expected tx, got {other:?}"),
        }
    }

    #[test]
    fn blob_tx_payload_round_trips() {
        let expected = sample_blob_tx();
        let decoded = round_trip_payload(Payload::BlobTx(sample_blob_tx()));
        match decoded {
            Payload::BlobTx(actual) => {
                assert_eq!(actual.owner, expected.owner);
                assert_eq!(actual.blob_id, expected.blob_id);
                assert_eq!(actual.blob_root, expected.blob_root);
                assert_eq!(actual.blob_size, expected.blob_size);
                assert_eq!(actual.fractal_lvl, expected.fractal_lvl);
                assert_eq!(actual.expiry, expected.expiry);
            }
            other => panic!("expected blob tx, got {other:?}"),
        }
    }

    #[test]
    fn block_payload_round_trips() {
        let expected = sample_block();
        let decoded = round_trip_payload(Payload::Block(7u16, sample_block()));
        match decoded {
            Payload::Block(shard, block) => {
                assert_eq!(shard, 7u16);
                assert_eq!(block.index, expected.index);
                assert_eq!(block.previous_hash, expected.previous_hash);
                assert_eq!(block.timestamp_millis, expected.timestamp_millis);
                assert_eq!(block.transactions.len(), expected.transactions.len());
                assert_eq!(block.difficulty, expected.difficulty);
                assert_eq!(block.retune_hint, expected.retune_hint);
                assert_eq!(block.nonce, expected.nonce);
                assert_eq!(block.hash, expected.hash);
                assert_eq!(
                    block.coinbase_consumer.get(),
                    expected.coinbase_consumer.get()
                );
                assert_eq!(
                    block.coinbase_industrial.get(),
                    expected.coinbase_industrial.get()
                );
                assert_eq!(block.storage_sub_ct.get(), expected.storage_sub_ct.get());
                assert_eq!(block.read_sub_ct.get(), expected.read_sub_ct.get());
                assert_eq!(block.compute_sub_ct.get(), expected.compute_sub_ct.get());
                assert_eq!(block.proof_rebate_ct.get(), expected.proof_rebate_ct.get());
                assert_eq!(block.storage_sub_it.get(), expected.storage_sub_it.get());
                assert_eq!(block.read_sub_it.get(), expected.read_sub_it.get());
                assert_eq!(block.compute_sub_it.get(), expected.compute_sub_it.get());
                assert_eq!(block.read_root, expected.read_root);
                assert_eq!(block.fee_checksum, expected.fee_checksum);
                assert_eq!(block.state_root, expected.state_root);
                assert_eq!(block.base_fee, expected.base_fee);
                assert_eq!(block.l2_roots, expected.l2_roots);
                assert_eq!(block.l2_sizes, expected.l2_sizes);
                assert_eq!(block.vdf_commit, expected.vdf_commit);
                assert_eq!(block.vdf_output, expected.vdf_output);
                assert_eq!(block.vdf_proof, expected.vdf_proof);
            }
            other => panic!("expected block, got {other:?}"),
        }
    }

    #[test]
    fn chain_payload_round_trips() {
        let decoded = round_trip_payload(Payload::Chain(vec![sample_block(), sample_block()]));
        match decoded {
            Payload::Chain(blocks) => {
                assert_eq!(blocks.len(), 2);
                assert_eq!(blocks[0].index, 1);
                assert_eq!(blocks[1].index, 1);
            }
            other => panic!("expected chain, got {other:?}"),
        }
    }

    #[test]
    fn blob_chunk_payload_round_trips() {
        let chunk = BlobChunk {
            root: [9u8; 32],
            index: 1,
            total: 10,
            data: Bytes::from(vec![1, 2, 3, 4]),
        };
        let decoded = round_trip_payload(Payload::BlobChunk(chunk.clone()));
        match decoded {
            Payload::BlobChunk(actual) => assert_eq!(actual, chunk),
            other => panic!("expected blob chunk, got {other:?}"),
        }
    }

    #[test]
    fn reputation_payload_round_trips() {
        let expected = sample_reputation();
        let decoded = round_trip_payload(Payload::Reputation(vec![sample_reputation()]));
        match decoded {
            Payload::Reputation(actual) => {
                assert_eq!(actual.len(), 1);
                assert_eq!(actual[0].provider_id, expected.provider_id);
                assert_eq!(actual[0].reputation_score, expected.reputation_score);
                assert_eq!(actual[0].epoch, expected.epoch);
            }
            other => panic!("expected reputation, got {other:?}"),
        }
    }

    #[test]
    fn message_round_trip_includes_optional_fields() {
        let payload = Payload::BlobTx(sample_blob_tx());
        let message = Message {
            pubkey: [8u8; 32],
            signature: Bytes::from(vec![3, 4, 5]),
            body: payload.clone(),
            partition: Some(11),
            cert_fingerprint: Some(Bytes::from(vec![0xde, 0xad])),
        };

        let encoded = encode_message(&message).expect("encode message");
        let mut reader = BinaryReader::new(&encoded);
        let decoded = read_message(&mut reader).expect("decode message");
        ensure_exhausted(&reader).expect("message exhausted");

        assert_eq!(decoded.pubkey, message.pubkey);
        assert_eq!(decoded.signature, message.signature);
        assert_eq!(decoded.partition, message.partition);
        assert_eq!(decoded.cert_fingerprint, message.cert_fingerprint);
        match (&decoded.body, payload) {
            (Payload::BlobTx(actual), Payload::BlobTx(expected)) => {
                assert_eq!(actual.owner, expected.owner);
                assert_eq!(actual.blob_id, expected.blob_id);
                assert_eq!(actual.blob_root, expected.blob_root);
                assert_eq!(actual.blob_size, expected.blob_size);
                assert_eq!(actual.fractal_lvl, expected.fractal_lvl);
                assert_eq!(actual.expiry, expected.expiry);
            }
            other => panic!("expected blob payload match, got {other:?}"),
        }
    }
}

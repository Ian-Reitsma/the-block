use std::convert::TryFrom;
use std::net::SocketAddr;

use crate::block_binary;
use crate::net::peer::ReputationUpdate;
use crate::p2p::handshake::Hello;
use crate::p2p::wire_binary;
use crate::storage::provider_directory::{
    ProviderAdvertisement, ProviderLookupRequest, ProviderLookupResponse,
};
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
    pub fn new(body: Payload, sk: &SigningKey) -> EncodeResult<Self> {
        Self::new_with_cert_fingerprint(body, sk, None)
    }

    pub fn new_with_cert_fingerprint(
        body: Payload,
        sk: &SigningKey,
        cert_fingerprint: Option<Bytes>,
    ) -> EncodeResult<Self> {
        let bytes = encode_payload(&body)?;
        let sig = sk.sign(&bytes);
        Ok(Self {
            pubkey: sk.verifying_key().to_bytes(),
            signature: Bytes::from(sig.to_bytes().to_vec()),
            body,
            partition: None,
            cert_fingerprint,
        })
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
    /// Request a chain snapshot when behind.
    ChainRequest(ChainRequest),
    /// Disseminate a single erasure-coded shard of a blob.
    BlobChunk(BlobChunk),
    /// Propagate provider reputation scores.
    Reputation(Vec<ReputationUpdate>),
    /// Advertise storage provider capabilities across the overlay.
    StorageProviderAdvertisement(ProviderAdvertisement),
    /// Request storage provider lookup via overlay (DHT-style).
    StorageProviderLookup(ProviderLookupRequest),
    /// Response carrying provider matches for a lookup.
    StorageProviderLookupResponse(ProviderLookupResponse),
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

/// Request a chain snapshot starting from a given height.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChainRequest {
    pub from_height: u64,
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
        signature: signature.ok_or(DecodeError::MissingField("signature"))?,
        body: body.ok_or(DecodeError::MissingField("body"))?,
        partition: partition.flatten(),
        cert_fingerprint: cert_fingerprint.flatten(),
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
        Payload::ChainRequest(request) => {
            writer.write_u32(8);
            write_chain_request(writer, request)?;
        }
        Payload::StorageProviderAdvertisement(advert) => {
            writer.write_u32(9);
            write_provider_advertisement(writer, advert)?;
        }
        Payload::StorageProviderLookup(request) => {
            writer.write_u32(10);
            write_provider_lookup_request(writer, request)?;
        }
        Payload::StorageProviderLookupResponse(response) => {
            writer.write_u32(11);
            write_provider_lookup_response(writer, response)?;
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
        8 => Ok(Payload::ChainRequest(read_chain_request(reader)?)),
        9 => Ok(Payload::StorageProviderAdvertisement(
            read_provider_advertisement(reader)?,
        )),
        10 => Ok(Payload::StorageProviderLookup(
            read_provider_lookup_request(reader)?,
        )),
        11 => Ok(Payload::StorageProviderLookupResponse(
            read_provider_lookup_response(reader)?,
        )),
        other => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "Payload",
            value: other,
        }),
    }
}

fn write_chain_request(writer: &mut BinaryWriter, request: &ChainRequest) -> EncodeResult<()> {
    writer.write_struct(|struct_writer| {
        struct_writer.field_u64("from_height", request.from_height);
    });
    Ok(())
}

fn read_chain_request(reader: &mut BinaryReader<'_>) -> binary_struct::Result<ChainRequest> {
    let mut from_height = None;

    decode_struct(reader, Some(1), |key, reader| match key {
        "from_height" => assign_once(&mut from_height, reader.read_u64()?, "from_height"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(ChainRequest {
        from_height: from_height.ok_or(DecodeError::MissingField("from_height"))?,
    })
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
        data: data.ok_or(DecodeError::MissingField("data"))?,
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

fn write_provider_advertisement(
    writer: &mut BinaryWriter,
    advert: &ProviderAdvertisement,
) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_with("profile", |field_writer| {
            if result.is_ok() {
                match storage_market::codec::serialize_provider_profile(&advert.profile) {
                    Ok(bytes) => {
                        if let Err(err) = write_bytes(
                            field_writer,
                            &Bytes::from(bytes),
                            "storage_provider_profile",
                        ) {
                            result = Err(err);
                        }
                    }
                    Err(_) => result = Err(EncodeError::LengthOverflow("storage_provider_profile")),
                }
            }
        });
        struct_writer.field_u64("version", advert.version);
        struct_writer.field_u64("ttl_secs", advert.ttl_secs);
        struct_writer.field_u64("expires_at", advert.expires_at);
        struct_writer.field_with("publisher", |field_writer| {
            if result.is_ok() {
                let bytes = Bytes::from(advert.publisher.to_vec());
                if let Err(err) = write_bytes(field_writer, &bytes, "publisher") {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("signature", |field_writer| {
            if result.is_ok() {
                if let Err(err) =
                    write_bytes(field_writer, &Bytes::from(advert.signature.clone()), "signature")
                {
                    result = Err(err);
                }
            }
        });
    });
    result
}

fn read_provider_advertisement(
    reader: &mut BinaryReader<'_>,
) -> binary_struct::Result<ProviderAdvertisement> {
    let mut profile = None;
    let mut version = None;
    let mut ttl_secs = None;
    let mut expires_at = None;
    let mut publisher = None;
    let mut signature = None;

    decode_struct(reader, Some(6), |key, reader| match key {
        "profile" => assign_once(&mut profile, read_bytes(reader)?, "profile"),
        "version" => assign_once(&mut version, reader.read_u64()?, "version"),
        "ttl_secs" => assign_once(&mut ttl_secs, reader.read_u64()?, "ttl_secs"),
        "expires_at" => assign_once(&mut expires_at, reader.read_u64()?, "expires_at"),
        "publisher" => assign_once(&mut publisher, read_bytes(reader)?, "publisher"),
        "signature" => assign_once(&mut signature, read_bytes(reader)?, "signature"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    let profile_bytes = profile.ok_or(DecodeError::MissingField("profile"))?;
    let profile = storage_market::codec::deserialize_provider_profile(&profile_bytes)
        .map_err(|err| DecodeError::InvalidFieldValue {
            field: "profile",
            reason: err.to_string(),
        })?;
    let publisher_bytes = publisher.ok_or(DecodeError::MissingField("publisher"))?;
    let publisher_arr: [u8; 32] = publisher_bytes
        .as_ref()
        .try_into()
        .map_err(|_| DecodeError::InvalidFieldValue {
            field: "publisher",
            reason: "expected 32 bytes".into(),
        })?;

    Ok(ProviderAdvertisement {
        profile,
        version: version.ok_or(DecodeError::MissingField("version"))?,
        ttl_secs: ttl_secs.ok_or(DecodeError::MissingField("ttl_secs"))?,
        expires_at: expires_at.ok_or(DecodeError::MissingField("expires_at"))?,
        publisher: publisher_arr,
        signature: signature
            .ok_or(DecodeError::MissingField("signature"))?
            .into_vec(),
    })
}

fn write_provider_lookup_request(
    writer: &mut BinaryWriter,
    request: &ProviderLookupRequest,
) -> EncodeResult<()> {
    writer.write_struct(|struct_writer| {
        struct_writer.field_u64("object_size", request.request.object_size);
        struct_writer.field_u16("shares", request.request.shares);
        struct_writer.field_option_string("region", request.request.region.as_ref());
        struct_writer.field_option_u64("max_price_per_block", request.request.max_price_per_block);
        struct_writer.field_option_u64(
            "min_success_rate_ppm",
            request.request.min_success_rate_ppm,
        );
        struct_writer.field_u64("limit", request.request.limit as u64);
        struct_writer.field_u64("nonce", request.nonce);
        struct_writer.field_u64("issued_at", request.issued_at);
        struct_writer.field_u8("ttl", request.ttl);
        struct_writer.field_with("origin", |field_writer| {
            write_fixed(field_writer, &request.origin);
        });
        struct_writer.field_with("signature", |field_writer| {
            write_bytes(field_writer, &request.signature, "signature")
        });
    });
    Ok(())
}

fn read_provider_lookup_request(
    reader: &mut BinaryReader<'_>,
) -> binary_struct::Result<ProviderLookupRequest> {
    let mut object_size = None;
    let mut shares = None;
    let mut region = None;
    let mut max_price_per_block = None;
    let mut min_success_rate_ppm = None;
    let mut limit = None;
    let mut nonce = None;
    let mut issued_at = None;
    let mut ttl = None;
    let mut origin = None;
    let mut signature = None;

    decode_struct(reader, Some(6), |key, reader| match key {
        "object_size" => assign_once(&mut object_size, reader.read_u64()?, "object_size"),
        "shares" => assign_once(&mut shares, reader.read_u16()?, "shares"),
        "region" => assign_once(&mut region, reader.read_option_string()?, "region"),
        "max_price_per_block" => assign_once(
            &mut max_price_per_block,
            reader.read_option_u64()?,
            "max_price_per_block",
        ),
        "min_success_rate_ppm" => assign_once(
            &mut min_success_rate_ppm,
            reader.read_option_u64()?,
            "min_success_rate_ppm",
        ),
        "limit" => assign_once(&mut limit, reader.read_u64()?, "limit"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "issued_at" => assign_once(&mut issued_at, reader.read_u64()?, "issued_at"),
        "ttl" => assign_once(&mut ttl, reader.read_u8()?, "ttl"),
        "origin" => assign_once(&mut origin, read_fixed(reader)?, "origin"),
        "signature" => assign_once(&mut signature, read_bytes(reader)?, "signature"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    let req = storage_market::DiscoveryRequest {
        object_size: object_size.ok_or(DecodeError::MissingField("object_size"))?,
        shares: shares.ok_or(DecodeError::MissingField("shares"))?,
        region,
        max_price_per_block,
        min_success_rate_ppm,
        limit: limit.ok_or(DecodeError::MissingField("limit"))? as usize,
    };

    Ok(ProviderLookupRequest {
        request: req,
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        issued_at: issued_at.ok_or(DecodeError::MissingField("issued_at"))?,
        ttl: ttl.ok_or(DecodeError::MissingField("ttl"))?,
        origin: origin.ok_or(DecodeError::MissingField("origin"))?,
        signature: signature.ok_or(DecodeError::MissingField("signature"))?,
    })
}

fn write_provider_lookup_response(
    writer: &mut BinaryWriter,
    response: &ProviderLookupResponse,
) -> EncodeResult<()> {
    writer.write_struct(|struct_writer| {
        struct_writer.field_u64("nonce", response.nonce);
        struct_writer.field_with("responder", |field_writer| {
            write_fixed(field_writer, &response.responder);
        });
        struct_writer.field_with("providers", |field_writer| {
            write_vec(
                field_writer,
                &response.providers,
                "providers",
                |writer, profile| storage_market::codec::write_provider_profile(writer, profile),
            )
        });
        struct_writer.field_with("signature", |field_writer| {
            write_bytes(field_writer, &response.signature, "signature")
        });
    });
    Ok(())
}

fn read_provider_lookup_response(
    reader: &mut BinaryReader<'_>,
) -> binary_struct::Result<ProviderLookupResponse> {
    let mut nonce = None;
    let mut responder = None;
    let mut providers: Option<Vec<storage_market::ProviderProfile>> = None;
    let mut signature = None;

    decode_struct(reader, Some(4), |key, reader| match key {
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "responder" => assign_once(&mut responder, read_fixed(reader)?, "responder"),
        "providers" => assign_once(
            &mut providers,
            read_vec(reader, |reader| storage_market::codec::read_provider_profile(reader))?,
            "providers",
        ),
        "signature" => assign_once(&mut signature, read_bytes(reader)?, "signature"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(ProviderLookupResponse {
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        responder: responder.ok_or(DecodeError::MissingField("responder"))?,
        providers: providers.ok_or(DecodeError::MissingField("providers"))?,
        signature: signature.ok_or(DecodeError::MissingField("signature"))?,
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
            pct: 64,
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
            coinbase_block: TokenAmount::new(10),
            coinbase_industrial: TokenAmount::new(11),
            storage_sub: TokenAmount::new(12),
            read_sub: TokenAmount::new(13),
            read_sub_viewer: TokenAmount::new(2),
            read_sub_host: TokenAmount::new(3),
            read_sub_hardware: TokenAmount::new(4),
            read_sub_verifier: TokenAmount::new(1),
            read_sub_liquidity: TokenAmount::new(3),
            ad_viewer: TokenAmount::new(5),
            ad_host: TokenAmount::new(6),
            ad_hardware: TokenAmount::new(7),
            ad_verifier: TokenAmount::new(8),
            ad_liquidity: TokenAmount::new(9),
            ad_miner: TokenAmount::new(10),
            treasury_events: Vec::new(),
            ad_total_usd_micros: 0,
            ad_settlement_count: 0,
            ad_oracle_price_usd_micros: 0,
            compute_sub: TokenAmount::new(14),
            proof_rebate: TokenAmount::new(15),
            read_root: [1u8; 32],
            fee_checksum: "fee".into(),
            state_root: "state".into(),
            root_bundles: Vec::new(),
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
            receipts: Vec::new(),
            receipt_header: None,
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
            gossip_addr: Some(SocketAddr::from(([127, 0, 0, 1], 8000))),
            quic_addr: Some(SocketAddr::from(([127, 0, 0, 1], 9000))),
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
            SocketAddr::from(([127, 0, 0, 1], 7000)),
            SocketAddr::from(([10, 0, 0, 5], 9000)),
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
                assert_eq!(block.coinbase_block.get(), expected.coinbase_block.get());
                assert_eq!(
                    block.coinbase_industrial.get(),
                    expected.coinbase_industrial.get()
                );
                assert_eq!(block.storage_sub.get(), expected.storage_sub.get());
                assert_eq!(block.read_sub.get(), expected.read_sub.get());
                assert_eq!(block.compute_sub.get(), expected.compute_sub.get());
                assert_eq!(block.proof_rebate.get(), expected.proof_rebate.get());
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
    fn chain_request_payload_round_trips() {
        let request = ChainRequest { from_height: 42 };
        let decoded = round_trip_payload(Payload::ChainRequest(request.clone()));
        match decoded {
            Payload::ChainRequest(actual) => assert_eq!(actual, request),
            other => panic!("expected chain request, got {other:?}"),
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

use std::convert::TryFrom;
use std::fmt;

use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::storage::types::{
    ChunkRef, ObjectManifest, ProviderChunkEntry, Redundancy, StoreReceipt,
};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted};

/// Result alias for encoding helpers.
pub type EncodeResult<T> = Result<T, EncodeError>;

/// Error returned when encoding fails.
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

/// Encode an [`ObjectManifest`] using the legacy binary layout.
pub fn encode_manifest(manifest: &ObjectManifest) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    write_manifest(&mut writer, manifest)?;
    Ok(writer.finish())
}

/// Decode an [`ObjectManifest`] from the legacy binary layout.
pub fn decode_manifest(bytes: &[u8]) -> binary_struct::Result<ObjectManifest> {
    let mut reader = Reader::new(bytes);
    let manifest = read_manifest(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(manifest)
}

/// Encode a [`StoreReceipt`] using the legacy binary layout.
pub fn encode_store_receipt(receipt: &StoreReceipt) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    writer.write_u64(4);
    writer.write_string("manifest_hash");
    write_fixed_u8_array(&mut writer, &receipt.manifest_hash);
    writer.write_string("chunk_count");
    writer.write_u32(receipt.chunk_count);
    writer.write_string("redundancy");
    write_redundancy(&mut writer, &receipt.redundancy)?;
    writer.write_string("lane");
    writer.write_string(&receipt.lane);
    Ok(writer.finish())
}

/// Decode a [`StoreReceipt`] from the legacy binary layout.
pub fn decode_store_receipt(bytes: &[u8]) -> binary_struct::Result<StoreReceipt> {
    let mut reader = Reader::new(bytes);
    let mut manifest_hash = None;
    let mut chunk_count = None;
    let mut redundancy = None;
    let mut lane = None;
    decode_struct(&mut reader, Some(4), |key, reader| match key {
        "manifest_hash" => {
            let value = read_fixed_u8_array(reader, 32)?;
            assign_once(&mut manifest_hash, value, "manifest_hash")
        }
        "chunk_count" => {
            let value = reader.read_u32()?;
            assign_once(&mut chunk_count, value, "chunk_count")
        }
        "redundancy" => {
            let value = read_redundancy(reader)?;
            assign_once(&mut redundancy, value, "redundancy")
        }
        "lane" => {
            let value = reader.read_string()?;
            assign_once(&mut lane, value, "lane")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    ensure_exhausted(&reader)?;
    Ok(StoreReceipt {
        manifest_hash: manifest_hash
            .ok_or(binary_struct::DecodeError::MissingField("manifest_hash"))?,
        chunk_count: chunk_count.ok_or(binary_struct::DecodeError::MissingField("chunk_count"))?,
        redundancy: redundancy.ok_or(binary_struct::DecodeError::MissingField("redundancy"))?,
        lane: lane.ok_or(binary_struct::DecodeError::MissingField("lane"))?,
    })
}

fn write_manifest(writer: &mut Writer, manifest: &ObjectManifest) -> EncodeResult<()> {
    writer.write_u64(15);
    writer.write_string("version");
    writer.write_u16(manifest.version);
    writer.write_string("total_len");
    writer.write_u64(manifest.total_len);
    writer.write_string("chunk_len");
    writer.write_u32(manifest.chunk_len);
    writer.write_string("chunks");
    write_chunk_refs(writer, &manifest.chunks)?;
    writer.write_string("redundancy");
    write_redundancy(writer, &manifest.redundancy)?;
    writer.write_string("content_key_enc");
    writer.write_bytes(&manifest.content_key_enc);
    writer.write_string("blake3");
    write_fixed_u8_array(writer, &manifest.blake3);
    writer.write_string("chunk_lens");
    write_u32_vec(writer, &manifest.chunk_lens)?;
    writer.write_string("chunk_compressed_lens");
    write_u32_vec(writer, &manifest.chunk_compressed_lens)?;
    writer.write_string("chunk_cipher_lens");
    write_u32_vec(writer, &manifest.chunk_cipher_lens)?;
    writer.write_string("compression_alg");
    write_option_string(writer, manifest.compression_alg.as_deref());
    writer.write_string("compression_level");
    write_option_i32(writer, manifest.compression_level);
    writer.write_string("encryption_alg");
    write_option_string(writer, manifest.encryption_alg.as_deref());
    writer.write_string("erasure_alg");
    write_option_string(writer, manifest.erasure_alg.as_deref());
    writer.write_string("provider_chunks");
    write_provider_chunk_entries(writer, &manifest.provider_chunks)?;
    Ok(())
}

fn read_manifest(reader: &mut Reader<'_>) -> binary_struct::Result<ObjectManifest> {
    let mut version = None;
    let mut total_len = None;
    let mut chunk_len = None;
    let mut chunks = None;
    let mut redundancy = None;
    let mut content_key_enc = None;
    let mut blake3 = None;
    let mut chunk_lens = None;
    let mut chunk_compressed_lens = None;
    let mut chunk_cipher_lens = None;
    let mut compression_alg: Option<Option<String>> = None;
    let mut compression_level: Option<Option<i32>> = None;
    let mut encryption_alg: Option<Option<String>> = None;
    let mut erasure_alg: Option<Option<String>> = None;
    let mut provider_chunks = None;
    decode_struct(reader, None, |key, reader| match key {
        "version" => {
            let value = reader.read_u16()?;
            assign_once(&mut version, value, "version")
        }
        "total_len" => {
            let value = reader.read_u64()?;
            assign_once(&mut total_len, value, "total_len")
        }
        "chunk_len" => {
            let value = reader.read_u32()?;
            assign_once(&mut chunk_len, value, "chunk_len")
        }
        "chunks" => {
            let value = read_chunk_refs(reader)?;
            assign_once(&mut chunks, value, "chunks")
        }
        "redundancy" => {
            let value = read_redundancy(reader)?;
            assign_once(&mut redundancy, value, "redundancy")
        }
        "content_key_enc" => {
            let value = reader.read_bytes()?;
            assign_once(&mut content_key_enc, value, "content_key_enc")
        }
        "blake3" => {
            let value = read_fixed_u8_array(reader, 32)?;
            assign_once(&mut blake3, value, "blake3")
        }
        "chunk_lens" => {
            let value = read_u32_vec(reader)?;
            assign_once(&mut chunk_lens, value, "chunk_lens")
        }
        "chunk_compressed_lens" => {
            let value = read_u32_vec(reader)?;
            assign_once(&mut chunk_compressed_lens, value, "chunk_compressed_lens")
        }
        "chunk_cipher_lens" => {
            let value = read_u32_vec(reader)?;
            assign_once(&mut chunk_cipher_lens, value, "chunk_cipher_lens")
        }
        "compression_alg" => {
            let value = reader.read_option_with(|reader| reader.read_string())?;
            assign_once(&mut compression_alg, value, "compression_alg")
        }
        "compression_level" => {
            let value = reader.read_option_with(|reader| {
                let raw = reader.read_i64()?;
                i32::try_from(raw).map_err(|_| binary_struct::DecodeError::InvalidFieldValue {
                    field: "compression_level",
                    reason: format!("value {raw} exceeds i32 range"),
                })
            })?;
            assign_once(&mut compression_level, value, "compression_level")
        }
        "encryption_alg" => {
            let value = reader.read_option_with(|reader| reader.read_string())?;
            assign_once(&mut encryption_alg, value, "encryption_alg")
        }
        "erasure_alg" => {
            let value = reader.read_option_with(|reader| reader.read_string())?;
            assign_once(&mut erasure_alg, value, "erasure_alg")
        }
        "provider_chunks" => {
            let value = read_provider_chunk_entries(reader)?;
            assign_once(&mut provider_chunks, value, "provider_chunks")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(ObjectManifest {
        version: version.ok_or(binary_struct::DecodeError::MissingField("version"))?,
        total_len: total_len.ok_or(binary_struct::DecodeError::MissingField("total_len"))?,
        chunk_len: chunk_len.ok_or(binary_struct::DecodeError::MissingField("chunk_len"))?,
        chunks: chunks.ok_or(binary_struct::DecodeError::MissingField("chunks"))?,
        redundancy: redundancy.ok_or(binary_struct::DecodeError::MissingField("redundancy"))?,
        content_key_enc: content_key_enc
            .ok_or(binary_struct::DecodeError::MissingField("content_key_enc"))?,
        blake3: blake3.ok_or(binary_struct::DecodeError::MissingField("blake3"))?,
        chunk_lens: chunk_lens.unwrap_or_default(),
        chunk_compressed_lens: chunk_compressed_lens.unwrap_or_default(),
        chunk_cipher_lens: chunk_cipher_lens.unwrap_or_default(),
        compression_alg: compression_alg.unwrap_or(None),
        compression_level: compression_level.unwrap_or(None),
        encryption_alg: encryption_alg.unwrap_or(None),
        erasure_alg: erasure_alg.unwrap_or(None),
        provider_chunks: provider_chunks.unwrap_or_default(),
    })
}

fn write_chunk_refs(writer: &mut Writer, chunks: &[ChunkRef]) -> EncodeResult<()> {
    write_vec(writer, chunks, "chunks", |writer, chunk| {
        write_chunk_ref(writer, chunk)
    })
}

fn write_chunk_ref(writer: &mut Writer, chunk: &ChunkRef) -> EncodeResult<()> {
    writer.write_u64(3);
    writer.write_string("id");
    write_fixed_u8_array(writer, &chunk.id);
    writer.write_string("nodes");
    write_string_vec(writer, &chunk.nodes)?;
    writer.write_string("provider_chunks");
    write_provider_chunk_entries(writer, &chunk.provider_chunks)?;
    Ok(())
}

fn write_provider_chunk_entries(
    writer: &mut Writer,
    entries: &[ProviderChunkEntry],
) -> EncodeResult<()> {
    write_vec(writer, entries, "provider_chunks", |writer, entry| {
        writer.write_u64(4);
        writer.write_string("provider");
        writer.write_string(&entry.provider);
        writer.write_string("chunk_indices");
        write_u32_vec(writer, &entry.chunk_indices)?;
        writer.write_string("chunk_lens");
        write_u32_vec(writer, &entry.chunk_lens)?;
        writer.write_string("encryption_key");
        writer.write_bytes(&entry.encryption_key);
        Ok(())
    })
}

fn write_redundancy(writer: &mut Writer, redundancy: &Redundancy) -> EncodeResult<()> {
    match redundancy {
        Redundancy::None => {
            writer.write_u32(0);
        }
        Redundancy::ReedSolomon { data, parity } => {
            writer.write_u32(1);
            writer.write_u64(2);
            writer.write_string("data");
            writer.write_u8(*data);
            writer.write_string("parity");
            writer.write_u8(*parity);
        }
    }
    Ok(())
}

fn write_u32_vec(writer: &mut Writer, values: &[u32]) -> EncodeResult<()> {
    write_vec(writer, values, "u32_vec", |writer, value| {
        writer.write_u32(*value);
        Ok(())
    })
}

fn write_string_vec(writer: &mut Writer, values: &[String]) -> EncodeResult<()> {
    write_vec(writer, values, "string_vec", |writer, value| {
        writer.write_string(value);
        Ok(())
    })
}

fn write_option_string(writer: &mut Writer, value: Option<&str>) {
    match value {
        Some(text) => {
            writer.write_bool(true);
            writer.write_string(text);
        }
        None => writer.write_bool(false),
    }
}

fn write_option_i32(writer: &mut Writer, value: Option<i32>) {
    match value {
        Some(v) => {
            writer.write_bool(true);
            writer.write_i64(v as i64);
        }
        None => writer.write_bool(false),
    }
}

fn write_fixed_u8_array(writer: &mut Writer, value: &[u8; 32]) {
    writer.write_u64(32);
    for byte in value {
        writer.write_u8(*byte);
    }
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

fn read_chunk_refs(reader: &mut Reader<'_>) -> binary_struct::Result<Vec<ChunkRef>> {
    let len = reader.read_u64()?;
    let len = usize::try_from(len)
        .map_err(|_| binary_struct::DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut chunks = Vec::with_capacity(len);
    for _ in 0..len {
        chunks.push(read_chunk_ref(reader)?);
    }
    Ok(chunks)
}

fn read_chunk_ref(reader: &mut Reader<'_>) -> binary_struct::Result<ChunkRef> {
    let mut id = None;
    let mut nodes = None;
    let mut provider_chunks = None;
    decode_struct(reader, None, |key, reader| match key {
        "id" => {
            let value = read_fixed_u8_array(reader, 32)?;
            assign_once(&mut id, value, "id")
        }
        "nodes" => {
            let value = read_string_vec(reader)?;
            assign_once(&mut nodes, value, "nodes")
        }
        "provider_chunks" => {
            let value = read_provider_chunk_entries(reader)?;
            assign_once(&mut provider_chunks, value, "provider_chunks")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(ChunkRef {
        id: id.ok_or(binary_struct::DecodeError::MissingField("id"))?,
        nodes: nodes.unwrap_or_default(),
        provider_chunks: provider_chunks.unwrap_or_default(),
    })
}

fn read_provider_chunk_entries(
    reader: &mut Reader<'_>,
) -> binary_struct::Result<Vec<ProviderChunkEntry>> {
    let len = reader.read_u64()?;
    let len = usize::try_from(len)
        .map_err(|_| binary_struct::DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut entries = Vec::with_capacity(len);
    for _ in 0..len {
        entries.push(read_provider_chunk_entry(reader)?);
    }
    Ok(entries)
}

fn read_provider_chunk_entry(reader: &mut Reader<'_>) -> binary_struct::Result<ProviderChunkEntry> {
    let mut provider = None;
    let mut chunk_indices = None;
    let mut chunk_lens = None;
    let mut encryption_key = None;
    decode_struct(reader, None, |key, reader| match key {
        "provider" => {
            let value = reader.read_string()?;
            assign_once(&mut provider, value, "provider")
        }
        "chunk_indices" => {
            let value = read_u32_vec(reader)?;
            assign_once(&mut chunk_indices, value, "chunk_indices")
        }
        "chunk_lens" => {
            let value = read_u32_vec(reader)?;
            assign_once(&mut chunk_lens, value, "chunk_lens")
        }
        "encryption_key" => {
            let value = reader.read_bytes()?;
            assign_once(&mut encryption_key, value, "encryption_key")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(ProviderChunkEntry {
        provider: provider.ok_or(binary_struct::DecodeError::MissingField("provider"))?,
        chunk_indices: chunk_indices.unwrap_or_default(),
        chunk_lens: chunk_lens.unwrap_or_default(),
        encryption_key: encryption_key.unwrap_or_default(),
    })
}

fn read_redundancy(reader: &mut Reader<'_>) -> binary_struct::Result<Redundancy> {
    let variant = reader.read_u32()?;
    match variant {
        0 => Ok(Redundancy::None),
        1 => {
            let mut data = None;
            let mut parity = None;
            decode_struct(reader, Some(2), |key, reader| match key {
                "data" => {
                    let value = reader.read_u8()?;
                    assign_once(&mut data, value, "data")
                }
                "parity" => {
                    let value = reader.read_u8()?;
                    assign_once(&mut parity, value, "parity")
                }
                other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
            })?;
            Ok(Redundancy::ReedSolomon {
                data: data.ok_or(binary_struct::DecodeError::MissingField("data"))?,
                parity: parity.ok_or(binary_struct::DecodeError::MissingField("parity"))?,
            })
        }
        other => Err(binary_struct::DecodeError::InvalidEnumDiscriminant {
            ty: "Redundancy",
            value: other,
        }),
    }
}

fn read_u32_vec(reader: &mut Reader<'_>) -> binary_struct::Result<Vec<u32>> {
    let len = reader.read_u64()?;
    let len = usize::try_from(len)
        .map_err(|_| binary_struct::DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(reader.read_u32()?);
    }
    Ok(values)
}

fn read_string_vec(reader: &mut Reader<'_>) -> binary_struct::Result<Vec<String>> {
    let len = reader.read_u64()?;
    let len = usize::try_from(len)
        .map_err(|_| binary_struct::DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(reader.read_string()?);
    }
    Ok(values)
}

fn read_fixed_u8_array(
    reader: &mut Reader<'_>,
    expected: usize,
) -> binary_struct::Result<[u8; 32]> {
    let len = reader.read_u64()?;
    let len_usize = usize::try_from(len)
        .map_err(|_| binary_struct::DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    if len_usize != expected {
        return Err(binary_struct::DecodeError::InvalidFieldValue {
            field: "fixed_array",
            reason: format!("expected length {expected} got {len_usize}"),
        });
    }
    let bytes = reader.read_exact(expected)?;
    let mut array = [0u8; 32];
    array.copy_from_slice(bytes);
    Ok(array)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::binary_codec;
    use std::ops::RangeInclusive;
    use testkit::{prop::Rng, tb_prop_test};

    #[test]
    fn manifest_binary_matches_legacy() {
        let manifest = sample_manifest();
        let encoded = encode_manifest(&manifest).expect("encode");
        let legacy = binary_codec::serialize(&manifest).expect("legacy encode");
        assert_eq!(encoded, legacy);

        let decoded = decode_manifest(&encoded).expect("decode");
        assert_eq!(decoded.version, manifest.version);
        assert_eq!(decoded.total_len, manifest.total_len);
        assert_eq!(decoded.chunk_len, manifest.chunk_len);
        assert_eq!(decoded.chunks.len(), manifest.chunks.len());
        assert_eq!(decoded.content_key_enc, manifest.content_key_enc);
        assert_eq!(decoded.blake3, manifest.blake3);
    }

    #[test]
    fn manifest_decode_handles_missing_optional_fields() {
        let mut manifest = sample_manifest();
        manifest.chunk_lens.clear();
        manifest.chunk_compressed_lens.clear();
        manifest.chunk_cipher_lens.clear();
        manifest.compression_alg = None;
        manifest.compression_level = None;
        manifest.encryption_alg = None;
        manifest.erasure_alg = None;
        manifest.provider_chunks.clear();

        let mut writer = Writer::new();
        writer.write_struct(|s| {
            s.field_with("version", |w| w.write_u16(manifest.version));
            s.field_with("total_len", |w| w.write_u64(manifest.total_len));
            s.field_with("chunk_len", |w| w.write_u32(manifest.chunk_len));
            s.field_with("chunks", |w| {
                write_chunk_refs(w, &manifest.chunks).expect("chunks")
            });
            s.field_with("redundancy", |w| {
                write_redundancy(w, &manifest.redundancy).expect("redundancy")
            });
            s.field_bytes("content_key_enc", &manifest.content_key_enc);
            s.field_with("blake3", |w| write_fixed_u8_array(w, &manifest.blake3));
        });
        let bytes = writer.finish();

        let decoded = decode_manifest(&bytes).expect("decode");
        assert_eq!(decoded.version, manifest.version);
        assert_eq!(decoded.total_len, manifest.total_len);
        assert_eq!(decoded.chunk_len, manifest.chunk_len);
        assert_eq!(decoded.chunks.len(), manifest.chunks.len());
        assert_eq!(decoded.chunk_lens.len(), 0);
        assert!(decoded.compression_alg.is_none());
        assert!(decoded.encryption_alg.is_none());
        assert!(decoded.erasure_alg.is_none());
        assert!(decoded.provider_chunks.is_empty());
    }

    #[test]
    fn manifest_binary_round_trips_complex_cases() {
        for manifest in complex_manifest_cases() {
            let encoded = encode_manifest(&manifest).expect("encode");
            let legacy = binary_codec::serialize(&manifest).expect("legacy encode");
            assert_eq!(encoded, legacy);

            let decoded = decode_manifest(&encoded).expect("decode");
            let reencoded = binary_codec::serialize(&decoded).expect("legacy reencode");
            assert_eq!(reencoded, legacy);
        }
    }

    #[test]
    fn receipt_binary_matches_legacy() {
        let receipt = StoreReceipt {
            manifest_hash: [5u8; 32],
            chunk_count: 3,
            redundancy: Redundancy::ReedSolomon { data: 4, parity: 2 },
            lane: "lane-a".to_string(),
        };
        let encoded = encode_store_receipt(&receipt).expect("encode");
        let legacy = binary_codec::serialize(&receipt).expect("legacy encode");
        assert_eq!(encoded, legacy);

        let decoded = decode_store_receipt(&encoded).expect("decode");
        assert_eq!(decoded.manifest_hash, receipt.manifest_hash);
        assert_eq!(decoded.chunk_count, receipt.chunk_count);
        match decoded.redundancy {
            Redundancy::ReedSolomon { data, parity } => {
                assert_eq!(data, 4);
                assert_eq!(parity, 2);
            }
            _ => panic!("unexpected redundancy"),
        }
        assert_eq!(decoded.lane, receipt.lane);
    }

    tb_prop_test!(manifest_binary_roundtrip_randomized, |runner| {
        runner
            .add_random_case("manifest roundtrip", 64, |rng| {
                let manifest = random_manifest(rng);
                let encoded = encode_manifest(&manifest).expect("encode");
                let legacy = binary_codec::serialize(&manifest).expect("legacy encode");
                assert_eq!(encoded, legacy);

                let decoded = decode_manifest(&encoded).expect("decode");
                assert_manifest_eq(&decoded, &manifest);
            })
            .expect("register manifest roundtrip case");

        runner
            .add_random_case("receipt roundtrip", 64, |rng| {
                let receipt = random_receipt(rng);
                let encoded = encode_store_receipt(&receipt).expect("encode");
                let legacy = binary_codec::serialize(&receipt).expect("legacy encode");
                assert_eq!(encoded, legacy);

                let decoded = decode_store_receipt(&encoded).expect("decode");
                assert_receipt_eq(&decoded, &receipt);
            })
            .expect("register receipt roundtrip case");
    });

    fn sample_manifest() -> ObjectManifest {
        let mut chunks = Vec::new();
        for idx in 0..2u8 {
            chunks.push(ChunkRef {
                id: [idx; 32],
                nodes: vec!["node-a".to_string(), "node-b".to_string()],
                provider_chunks: vec![ProviderChunkEntry {
                    provider: "prov".to_string(),
                    chunk_indices: vec![1, 2],
                    chunk_lens: vec![100, 200],
                    encryption_key: vec![9, 8, 7],
                }],
            });
        }
        ObjectManifest {
            version: 1,
            total_len: 2048,
            chunk_len: 1024,
            chunks,
            redundancy: Redundancy::ReedSolomon { data: 6, parity: 3 },
            content_key_enc: vec![1, 2, 3, 4],
            blake3: [7u8; 32],
            chunk_lens: vec![1024, 1024],
            chunk_compressed_lens: vec![900, 900],
            chunk_cipher_lens: vec![1100, 1100],
            compression_alg: Some("gzip".to_string()),
            compression_level: Some(3),
            encryption_alg: Some("aes".to_string()),
            erasure_alg: Some("reed_solomon".to_string()),
            provider_chunks: vec![ProviderChunkEntry {
                provider: "prov".to_string(),
                chunk_indices: vec![0],
                chunk_lens: vec![1024],
                encryption_key: vec![5, 5, 5],
            }],
        }
    }

    fn complex_manifest_cases() -> Vec<ObjectManifest> {
        let mut cases = Vec::new();

        // Baseline manifest with redundancy and optional metadata populated.
        cases.push(sample_manifest());

        // Manifest using Redundancy::None and empty optional metadata.
        let mut none_manifest = sample_manifest();
        none_manifest.redundancy = Redundancy::None;
        none_manifest.chunk_lens.clear();
        none_manifest.chunk_compressed_lens.clear();
        none_manifest.chunk_cipher_lens.clear();
        none_manifest.compression_alg = None;
        none_manifest.compression_level = None;
        none_manifest.encryption_alg = None;
        none_manifest.erasure_alg = None;
        none_manifest.provider_chunks.clear();
        cases.push(none_manifest);

        // Manifest with large chunk/provider tables to stress length handling.
        let mut large_manifest = sample_manifest();
        large_manifest.chunks.clear();
        large_manifest.chunk_lens.clear();
        large_manifest.chunk_compressed_lens.clear();
        large_manifest.chunk_cipher_lens.clear();
        large_manifest.provider_chunks.clear();
        let mut chunk_lens = Vec::new();
        for idx in 0..64u32 {
            let mut id = [0u8; 32];
            id[0] = (idx & 0xFF) as u8;
            id[1] = ((idx >> 8) & 0xFF) as u8;
            large_manifest.chunks.push(ChunkRef {
                id,
                nodes: vec![format!("node-{idx}"), format!("backup-{idx}")],
                provider_chunks: vec![ProviderChunkEntry {
                    provider: format!("provider-{idx}"),
                    chunk_indices: vec![idx, idx + 1, idx + 2],
                    chunk_lens: vec![128 + idx, 256 + idx],
                    encryption_key: vec![idx as u8; 4],
                }],
            });
            chunk_lens.push(2048 + idx);
        }
        large_manifest.chunk_lens = chunk_lens.clone();
        large_manifest.chunk_compressed_lens = chunk_lens.clone();
        large_manifest.chunk_cipher_lens = chunk_lens;
        large_manifest.provider_chunks = large_manifest
            .chunks
            .iter()
            .map(|chunk| ProviderChunkEntry {
                provider: format!("{}-mirror", chunk.nodes[0]),
                chunk_indices: vec![1, 3, 5, 7],
                chunk_lens: vec![512, 1024, 1536],
                encryption_key: vec![0xA5, 0x5A],
            })
            .collect();
        cases.push(large_manifest);

        cases
    }

    fn random_manifest(rng: &mut Rng) -> ObjectManifest {
        let chunk_count = rng.range_usize(0..=24);
        let chunk_len = rng.range_u32(1..=16_384);
        let total_len = if chunk_count == 0 {
            0
        } else {
            let base = (chunk_len as u64) * (chunk_count as u64);
            base.saturating_add(rng.range_u64(0..=chunk_len as u64))
        };

        let chunks: Vec<ChunkRef> = (0..chunk_count)
            .map(|_| random_chunk_ref(rng, chunk_count))
            .collect();

        let chunk_lens = maybe_vec(rng, chunk_count, |rng| rng.range_u32(1..=chunk_len));
        let chunk_compressed_lens = maybe_vec(rng, chunk_count, |rng| {
            let base = rng.range_u32(1..=chunk_len);
            base.min(chunk_len)
        });
        let chunk_cipher_lens = maybe_vec(rng, chunk_count, |rng| {
            let plain = rng.range_u32(1..=chunk_len.saturating_add(4096));
            plain
        });

        let redundancy = if rng.bool() {
            Redundancy::None
        } else {
            Redundancy::ReedSolomon {
                data: rng.range_u8(1..=32),
                parity: rng.range_u8(1..=16),
            }
        };

        let provider_chunks = if rng.bool() {
            Vec::new()
        } else {
            let entry_count = rng.range_usize(0..=chunk_count.saturating_add(4));
            (0..entry_count)
                .map(|_| random_provider_entry(rng, chunk_count))
                .collect()
        };

        ObjectManifest {
            version: rng.range_u16(0..=u16::MAX),
            total_len,
            chunk_len,
            chunks,
            redundancy,
            content_key_enc: rng.bytes(0..=96),
            blake3: random_array32(rng),
            chunk_lens,
            chunk_compressed_lens,
            chunk_cipher_lens,
            compression_alg: maybe_option_string(rng, 0..=12),
            compression_level: maybe_option_i32(rng),
            encryption_alg: maybe_option_string(rng, 0..=12),
            erasure_alg: maybe_option_string(rng, 0..=12),
            provider_chunks,
        }
    }

    fn random_chunk_ref(rng: &mut Rng, chunk_count: usize) -> ChunkRef {
        let node_count = rng.range_usize(0..=4);
        let nodes = (0..node_count)
            .map(|_| random_identifier(rng, 4..=12))
            .collect();
        let provider_chunks = if rng.bool() {
            Vec::new()
        } else {
            let entry_count = rng.range_usize(0..=3);
            (0..entry_count)
                .map(|_| random_provider_entry(rng, chunk_count))
                .collect()
        };

        ChunkRef {
            id: random_array32(rng),
            nodes,
            provider_chunks,
        }
    }

    fn random_provider_entry(rng: &mut Rng, chunk_count: usize) -> ProviderChunkEntry {
        let index_count = rng.range_usize(0..=chunk_count.saturating_add(1));
        let mut chunk_indices = Vec::with_capacity(index_count);
        for _ in 0..index_count {
            if chunk_count == 0 {
                break;
            }
            chunk_indices.push(rng.range_u32(0..=((chunk_count - 1) as u32)));
        }

        let lens = maybe_vec_from(rng, chunk_indices.len(), |rng| rng.range_u32(1..=65_535));

        ProviderChunkEntry {
            provider: random_identifier(rng, 3..=18),
            chunk_indices,
            chunk_lens: lens,
            encryption_key: rng.bytes(0..=64),
        }
    }

    fn random_receipt(rng: &mut Rng) -> StoreReceipt {
        StoreReceipt {
            manifest_hash: random_array32(rng),
            chunk_count: rng.range_u32(0..=65_535),
            redundancy: if rng.bool() {
                Redundancy::None
            } else {
                Redundancy::ReedSolomon {
                    data: rng.range_u8(1..=24),
                    parity: rng.range_u8(1..=12),
                }
            },
            lane: random_identifier(rng, 3..=24),
        }
    }

    fn assert_manifest_eq(actual: &ObjectManifest, expected: &ObjectManifest) {
        assert_eq!(actual.version, expected.version, "version mismatch");
        assert_eq!(actual.total_len, expected.total_len, "total_len mismatch");
        assert_eq!(actual.chunk_len, expected.chunk_len, "chunk_len mismatch");
        assert_eq!(actual.chunks.len(), expected.chunks.len(), "chunks len");
        for (idx, (lhs, rhs)) in actual.chunks.iter().zip(expected.chunks.iter()).enumerate() {
            assert_eq!(lhs.id, rhs.id, "chunk id {idx}");
            assert_eq!(lhs.nodes, rhs.nodes, "chunk nodes {idx}");
            assert_eq!(
                lhs.provider_chunks.len(),
                rhs.provider_chunks.len(),
                "chunk providers {idx}"
            );
            for (entry_idx, (lhs_entry, rhs_entry)) in lhs
                .provider_chunks
                .iter()
                .zip(rhs.provider_chunks.iter())
                .enumerate()
            {
                assert_eq!(
                    lhs_entry.provider, rhs_entry.provider,
                    "provider {idx}:{entry_idx}"
                );
                assert_eq!(
                    lhs_entry.chunk_indices, rhs_entry.chunk_indices,
                    "indices {idx}:{entry_idx}"
                );
                assert_eq!(
                    lhs_entry.chunk_lens, rhs_entry.chunk_lens,
                    "lens {idx}:{entry_idx}"
                );
                assert_eq!(
                    lhs_entry.encryption_key, rhs_entry.encryption_key,
                    "key {idx}:{entry_idx}"
                );
            }
        }
        assert_eq!(
            actual.redundancy, expected.redundancy,
            "redundancy mismatch"
        );
        assert_eq!(
            actual.content_key_enc, expected.content_key_enc,
            "content key"
        );
        assert_eq!(actual.blake3, expected.blake3, "blake3");
        assert_eq!(actual.chunk_lens, expected.chunk_lens, "chunk lens");
        assert_eq!(
            actual.chunk_compressed_lens, expected.chunk_compressed_lens,
            "compressed lens",
        );
        assert_eq!(
            actual.chunk_cipher_lens, expected.chunk_cipher_lens,
            "cipher lens",
        );
        assert_eq!(
            actual.compression_alg, expected.compression_alg,
            "compression alg"
        );
        assert_eq!(
            actual.compression_level, expected.compression_level,
            "compression level"
        );
        assert_eq!(
            actual.encryption_alg, expected.encryption_alg,
            "encryption alg"
        );
        assert_eq!(actual.erasure_alg, expected.erasure_alg, "erasure alg");
        assert_eq!(
            actual.provider_chunks.len(),
            expected.provider_chunks.len(),
            "provider chunks len"
        );
        for (idx, (lhs, rhs)) in actual
            .provider_chunks
            .iter()
            .zip(expected.provider_chunks.iter())
            .enumerate()
        {
            assert_eq!(lhs.provider, rhs.provider, "provider chunk provider {idx}");
            assert_eq!(
                lhs.chunk_indices, rhs.chunk_indices,
                "provider chunk indices {idx}"
            );
            assert_eq!(lhs.chunk_lens, rhs.chunk_lens, "provider chunk lens {idx}");
            assert_eq!(
                lhs.encryption_key, rhs.encryption_key,
                "provider chunk key {idx}"
            );
        }
    }

    fn assert_receipt_eq(actual: &StoreReceipt, expected: &StoreReceipt) {
        assert_eq!(
            actual.manifest_hash, expected.manifest_hash,
            "manifest hash"
        );
        assert_eq!(actual.chunk_count, expected.chunk_count, "chunk count");
        assert_eq!(actual.redundancy, expected.redundancy, "redundancy");
        assert_eq!(actual.lane, expected.lane, "lane");
    }

    fn random_identifier(rng: &mut Rng, len_range: RangeInclusive<usize>) -> String {
        let len = rng.range_usize(len_range.clone());
        let mut out = String::with_capacity(len);
        for _ in 0..len {
            let ch = rng.range_u8(b'a'..=b'z');
            out.push(char::from(ch));
        }
        out
    }

    fn random_array32(rng: &mut Rng) -> [u8; 32] {
        let bytes = rng.bytes(32..=32);
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        array
    }

    fn maybe_vec<T, F>(rng: &mut Rng, len: usize, mut gen: F) -> Vec<T>
    where
        F: FnMut(&mut Rng) -> T,
    {
        if len == 0 || rng.bool() {
            return Vec::new();
        }
        (0..len).map(|_| gen(rng)).collect()
    }

    fn maybe_vec_from<T, F>(rng: &mut Rng, len: usize, mut gen: F) -> Vec<T>
    where
        F: FnMut(&mut Rng) -> T,
    {
        if len == 0 {
            return Vec::new();
        }
        if rng.bool() {
            (0..len).map(|_| gen(rng)).collect()
        } else {
            Vec::new()
        }
    }

    fn maybe_option_string(rng: &mut Rng, len_range: RangeInclusive<usize>) -> Option<String> {
        if rng.bool() {
            Some(random_identifier(rng, len_range))
        } else {
            None
        }
    }

    fn maybe_option_i32(rng: &mut Rng) -> Option<i32> {
        if rng.bool() {
            let magnitude = rng.range_u32(0..=10_000) as i32;
            if rng.bool() {
                Some(magnitude)
            } else {
                Some(-magnitude)
            }
        } else {
            None
        }
    }
}

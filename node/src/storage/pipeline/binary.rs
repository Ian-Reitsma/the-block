use std::fmt;

use foundation_serialization::binary_cursor::{Reader, Writer};

use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted};

use super::ProviderProfile;

/// Result alias for provider profile encoding helpers.
pub type EncodeResult<T> = Result<T, EncodeError>;

/// Error raised when encoding fails.
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

/// Encode a [`ProviderProfile`] using the legacy binary layout.
pub fn encode_provider_profile(profile: &ProviderProfile) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    writer.write_u64(13);
    writer.write_string("bw_ewma");
    writer.write_f64(profile.bw_ewma);
    writer.write_string("rtt_ewma");
    writer.write_f64(profile.rtt_ewma);
    writer.write_string("loss_ewma");
    writer.write_f64(profile.loss_ewma);
    writer.write_string("preferred_chunk");
    writer.write_u32(profile.preferred_chunk);
    writer.write_string("stable_chunks");
    writer.write_u32(profile.stable_chunks);
    writer.write_string("updated_at");
    writer.write_u64(profile.updated_at);
    writer.write_string("success_rate_ewma");
    writer.write_f64(profile.success_rate_ewma);
    writer.write_string("recent_failures");
    writer.write_u32(profile.recent_failures);
    writer.write_string("total_chunks");
    writer.write_u64(profile.total_chunks);
    writer.write_string("total_failures");
    writer.write_u64(profile.total_failures);
    writer.write_string("last_upload_bytes");
    writer.write_u64(profile.last_upload_bytes);
    writer.write_string("last_upload_secs");
    writer.write_f64(profile.last_upload_secs);
    writer.write_string("maintenance");
    writer.write_bool(profile.maintenance);
    Ok(writer.finish())
}

/// Decode a [`ProviderProfile`] from the legacy binary layout.
pub fn decode_provider_profile(bytes: &[u8]) -> binary_struct::Result<ProviderProfile> {
    let mut reader = Reader::new(bytes);
    let mut builder = ProviderProfileBuilder::default();
    decode_struct(&mut reader, None, |key, reader| match key {
        "bw_ewma" => {
            let value = reader.read_f64()?;
            assign_once(&mut builder.bw_ewma, value, "bw_ewma")
        }
        "rtt_ewma" => {
            let value = reader.read_f64()?;
            assign_once(&mut builder.rtt_ewma, value, "rtt_ewma")
        }
        "loss_ewma" => {
            let value = reader.read_f64()?;
            assign_once(&mut builder.loss_ewma, value, "loss_ewma")
        }
        "preferred_chunk" => {
            let value = reader.read_u32()?;
            assign_once(&mut builder.preferred_chunk, value, "preferred_chunk")
        }
        "stable_chunks" => {
            let value = reader.read_u32()?;
            assign_once(&mut builder.stable_chunks, value, "stable_chunks")
        }
        "updated_at" => {
            let value = reader.read_u64()?;
            assign_once(&mut builder.updated_at, value, "updated_at")
        }
        "success_rate_ewma" => {
            let value = reader.read_f64()?;
            assign_once(&mut builder.success_rate_ewma, value, "success_rate_ewma")
        }
        "recent_failures" => {
            let value = reader.read_u32()?;
            assign_once(&mut builder.recent_failures, value, "recent_failures")
        }
        "total_chunks" => {
            let value = reader.read_u64()?;
            assign_once(&mut builder.total_chunks, value, "total_chunks")
        }
        "total_failures" => {
            let value = reader.read_u64()?;
            assign_once(&mut builder.total_failures, value, "total_failures")
        }
        "last_upload_bytes" => {
            let value = reader.read_u64()?;
            assign_once(&mut builder.last_upload_bytes, value, "last_upload_bytes")
        }
        "last_upload_secs" => {
            let value = reader.read_f64()?;
            assign_once(&mut builder.last_upload_secs, value, "last_upload_secs")
        }
        "maintenance" => {
            let value = reader.read_bool()?;
            assign_once(&mut builder.maintenance, value, "maintenance")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    ensure_exhausted(&reader)?;
    builder.finish()
}

#[derive(Default)]
struct ProviderProfileBuilder {
    bw_ewma: Option<f64>,
    rtt_ewma: Option<f64>,
    loss_ewma: Option<f64>,
    preferred_chunk: Option<u32>,
    stable_chunks: Option<u32>,
    updated_at: Option<u64>,
    success_rate_ewma: Option<f64>,
    recent_failures: Option<u32>,
    total_chunks: Option<u64>,
    total_failures: Option<u64>,
    last_upload_bytes: Option<u64>,
    last_upload_secs: Option<f64>,
    maintenance: Option<bool>,
}

impl ProviderProfileBuilder {
    fn finish(self) -> binary_struct::Result<ProviderProfile> {
        let mut profile = ProviderProfile::new();
        if let Some(value) = self.bw_ewma {
            profile.bw_ewma = value;
        }
        if let Some(value) = self.rtt_ewma {
            profile.rtt_ewma = value;
        }
        if let Some(value) = self.loss_ewma {
            profile.loss_ewma = value;
        }
        if let Some(value) = self.preferred_chunk {
            profile.preferred_chunk = value;
        }
        if let Some(value) = self.stable_chunks {
            profile.stable_chunks = value;
        }
        if let Some(value) = self.updated_at {
            profile.updated_at = value;
        }
        if let Some(value) = self.success_rate_ewma {
            profile.success_rate_ewma = value;
        }
        if let Some(value) = self.recent_failures {
            profile.recent_failures = value;
        }
        if let Some(value) = self.total_chunks {
            profile.total_chunks = value;
        }
        if let Some(value) = self.total_failures {
            profile.total_failures = value;
        }
        if let Some(value) = self.last_upload_bytes {
            profile.last_upload_bytes = value;
        }
        if let Some(value) = self.last_upload_secs {
            profile.last_upload_secs = value;
        }
        if let Some(value) = self.maintenance {
            profile.maintenance = value;
        }
        profile.ensure_defaults();
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::binary_codec;
    use testkit::{prop::Rng, tb_prop_test};

    #[test]
    fn provider_profile_binary_matches_legacy() {
        let mut profile = ProviderProfile::new();
        profile.bw_ewma = 12.5;
        profile.rtt_ewma = 7.25;
        profile.loss_ewma = 0.01;
        profile.preferred_chunk = 512 * 1024;
        profile.stable_chunks = 4;
        profile.updated_at = 4242;
        profile.success_rate_ewma = 0.9;
        profile.recent_failures = 3;
        profile.total_chunks = 128;
        profile.total_failures = 5;
        profile.last_upload_bytes = 4096;
        profile.last_upload_secs = 1.25;
        profile.maintenance = true;

        let encoded = encode_provider_profile(&profile).expect("encode");
        let legacy = binary_codec::serialize(&profile).expect("legacy encode");
        assert_eq!(encoded, legacy);

        let decoded = decode_provider_profile(&encoded).expect("decode");
        assert_eq!(decoded, profile);
    }

    #[test]
    fn provider_profile_decode_handles_missing_optional_fields() {
        let mut writer = Writer::new();
        writer.write_struct(|s| {
            s.field_f64("bw_ewma", 5.0);
            s.field_f64("rtt_ewma", 15.0);
            s.field_f64("loss_ewma", 0.25);
            s.field_u64("updated_at", 99);
        });
        let bytes = writer.finish();

        let decoded = decode_provider_profile(&bytes).expect("decode");
        assert_eq!(decoded.bw_ewma, 5.0);
        assert_eq!(decoded.rtt_ewma, 15.0);
        assert_eq!(decoded.loss_ewma, 0.25);
        assert_eq!(decoded.updated_at, 99);
        assert_eq!(decoded.preferred_chunk, super::super::default_chunk_size());
        assert_eq!(decoded.recent_failures, 0);
        assert_eq!(decoded.total_chunks, 0);
        assert_eq!(decoded.total_failures, 0);
        assert_eq!(decoded.last_upload_bytes, 0);
        assert_eq!(decoded.last_upload_secs, 0.0);
        assert!(!decoded.maintenance);
    }

    tb_prop_test!(provider_profile_roundtrip_randomized, |runner| {
        runner
            .add_random_case("profile roundtrip", 64, |rng| {
                let profile = random_profile(rng);
                let encoded = encode_provider_profile(&profile).expect("encode");
                let legacy = binary_codec::serialize(&profile).expect("legacy encode");
                assert_eq!(encoded, legacy);

                let decoded = decode_provider_profile(&encoded).expect("decode");
                assert_eq!(decoded, normalize_profile(&profile));
            })
            .expect("register provider profile case");
    });

    fn random_profile(rng: &mut Rng) -> ProviderProfile {
        let mut profile = ProviderProfile::new();
        profile.bw_ewma = random_rate(rng);
        profile.rtt_ewma = random_rate(rng);
        profile.loss_ewma = random_rate(rng);
        profile.preferred_chunk = if rng.bool() {
            0
        } else {
            rng.range_u32(256..=131_072)
        };
        profile.stable_chunks = rng.range_u32(0..=10_000);
        profile.updated_at = rng.range_u64(0..=u64::MAX / 2);
        profile.success_rate_ewma = random_rate(rng);
        profile.recent_failures = rng.range_u32(0..=10_000);
        profile.total_chunks = rng.range_u64(0..=5_000_000);
        profile.total_failures = rng.range_u64(0..=250_000);
        profile.last_upload_bytes = rng.range_u64(0..=10_000_000);
        profile.last_upload_secs = random_rate(rng);
        profile.maintenance = rng.bool();
        profile
    }

    fn random_rate(rng: &mut Rng) -> f64 {
        let raw = rng.range_u64(0..=1_000_000);
        let magnitude = (raw as f64) / 1_000.0;
        if rng.bool() {
            magnitude
        } else {
            -magnitude
        }
    }

    fn normalize_profile(profile: &ProviderProfile) -> ProviderProfile {
        let mut normalized = profile.clone();
        normalized.ensure_defaults();
        normalized
    }
}

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
const PROFILE_FIELD_COUNT: usize = 13;

pub fn encode_provider_profile(profile: &ProviderProfile) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    write_field_count(&mut writer, PROFILE_FIELD_COUNT)?;
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

fn write_field_count(writer: &mut Writer, count: usize) -> EncodeResult<()> {
    let len = convert_field_count(count as u128)?;
    writer.write_u64(len);
    Ok(())
}

fn convert_field_count(count: u128) -> EncodeResult<u64> {
    if count > u64::MAX as u128 {
        Err(EncodeError::LengthOverflow("profile_fields"))
    } else {
        Ok(count as u64)
    }
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

    const PROVIDER_PROFILE_CURSOR_FIXTURE: &[u8] = &[
        13, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 98, 119, 95, 101, 119, 109, 97, 0, 0, 0,
        0, 0, 0, 248, 63, 8, 0, 0, 0, 0, 0, 0, 0, 114, 116, 116, 95, 101, 119, 109, 97, 0, 0, 0, 0,
        0, 64, 111, 64, 9, 0, 0, 0, 0, 0, 0, 0, 108, 111, 115, 115, 95, 101, 119, 109, 97, 154,
        153, 153, 153, 153, 153, 169, 63, 15, 0, 0, 0, 0, 0, 0, 0, 112, 114, 101, 102, 101, 114,
        114, 101, 100, 95, 99, 104, 117, 110, 107, 0, 16, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0, 115, 116,
        97, 98, 108, 101, 95, 99, 104, 117, 110, 107, 115, 12, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0,
        117, 112, 100, 97, 116, 101, 100, 95, 97, 116, 64, 105, 209, 102, 0, 0, 0, 0, 17, 0, 0, 0,
        0, 0, 0, 0, 115, 117, 99, 99, 101, 115, 115, 95, 114, 97, 116, 101, 95, 101, 119, 109, 97,
        205, 204, 204, 204, 204, 204, 236, 63, 15, 0, 0, 0, 0, 0, 0, 0, 114, 101, 99, 101, 110,
        116, 95, 102, 97, 105, 108, 117, 114, 101, 115, 2, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 116,
        111, 116, 97, 108, 95, 99, 104, 117, 110, 107, 115, 0, 2, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0, 0,
        0, 0, 0, 116, 111, 116, 97, 108, 95, 102, 97, 105, 108, 117, 114, 101, 115, 8, 0, 0, 0, 0,
        0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0, 108, 97, 115, 116, 95, 117, 112, 108, 111, 97, 100, 95,
        98, 121, 116, 101, 115, 0, 32, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 108, 97, 115,
        116, 95, 117, 112, 108, 111, 97, 100, 95, 115, 101, 99, 115, 0, 0, 0, 0, 0, 0, 10, 64, 11,
        0, 0, 0, 0, 0, 0, 0, 109, 97, 105, 110, 116, 101, 110, 97, 110, 99, 101, 1,
    ];

    fn sample_profile() -> ProviderProfile {
        ProviderProfile {
            bw_ewma: 1.5,
            rtt_ewma: 250.0,
            loss_ewma: 0.05,
            preferred_chunk: 4096,
            stable_chunks: 12,
            updated_at: 1_725_000_000,
            success_rate_ewma: 0.9,
            recent_failures: 2,
            total_chunks: 512,
            total_failures: 8,
            last_upload_bytes: 8192,
            last_upload_secs: 3.25,
            maintenance: true,
        }
    }

    fn with_first_party_only_env<R>(value: Option<&str>, f: impl FnOnce() -> R) -> R {
        static GUARD: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let lock = GUARD
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env guard");

        let original = std::env::var("FIRST_PARTY_ONLY").ok();
        match value {
            Some(v) => std::env::set_var("FIRST_PARTY_ONLY", v),
            None => std::env::remove_var("FIRST_PARTY_ONLY"),
        }

        let result = f();

        match original {
            Some(v) => std::env::set_var("FIRST_PARTY_ONLY", v),
            None => std::env::remove_var("FIRST_PARTY_ONLY"),
        }

        drop(lock);
        result
    }

    #[test]
    fn provider_profile_cursor_roundtrip_matches_fixture() {
        let profile = sample_profile();
        let encoded =
            encode_provider_profile(&profile).expect("encode provider profile via cursor");
        if PROVIDER_PROFILE_CURSOR_FIXTURE.is_empty() {
            panic!("fixture pending: {:?}", encoded);
        }
        assert_eq!(encoded, PROVIDER_PROFILE_CURSOR_FIXTURE);

        let decoded =
            decode_provider_profile(PROVIDER_PROFILE_CURSOR_FIXTURE).expect("decode profile");
        assert_eq!(decoded.bw_ewma, profile.bw_ewma);
        assert_eq!(decoded.rtt_ewma, profile.rtt_ewma);
        assert_eq!(decoded.loss_ewma, profile.loss_ewma);
        assert_eq!(decoded.preferred_chunk, profile.preferred_chunk);
        assert_eq!(decoded.stable_chunks, profile.stable_chunks);
        assert_eq!(decoded.updated_at, profile.updated_at);
        assert_eq!(decoded.success_rate_ewma, profile.success_rate_ewma);
        assert_eq!(decoded.recent_failures, profile.recent_failures);
        assert_eq!(decoded.total_chunks, profile.total_chunks);
        assert_eq!(decoded.total_failures, profile.total_failures);
        assert_eq!(decoded.last_upload_bytes, profile.last_upload_bytes);
        assert!((decoded.last_upload_secs - profile.last_upload_secs).abs() < f64::EPSILON);
        assert_eq!(decoded.maintenance, profile.maintenance);
    }

    #[test]
    fn provider_profile_cursor_roundtrip_respects_first_party_only_flag() {
        let profile = sample_profile();
        for flag in [Some("1"), Some("0"), None] {
            with_first_party_only_env(flag, || {
                let encoded =
                    encode_provider_profile(&profile).expect("encode provider profile with flag");
                let decoded =
                    decode_provider_profile(&encoded).expect("decode provider profile with flag");
                assert_eq!(decoded.preferred_chunk, profile.preferred_chunk);
                assert_eq!(decoded.maintenance, profile.maintenance);
            });
        }
    }

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

    #[test]
    fn write_field_count_rejects_overflow() {
        let err =
            super::convert_field_count(u64::MAX as u128 + 1).expect_err("overflow must error");
        match err {
            EncodeError::LengthOverflow(field) => assert_eq!(field, "profile_fields"),
        }
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

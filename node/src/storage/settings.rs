use std::env;
use std::path::Path;
use std::sync::{Arc, RwLock};

use coding::{
    compressor_for, encryptor_for, erasure_coder_for, fountain_coder_for, CodingError, Compressor,
    Config, ConfigError, Encryptor, ErasureCoder, FountainCoder, DEFAULT_FALLBACK_EMERGENCY_ENV,
};
use once_cell::sync::Lazy;

#[derive(Clone, Debug)]
pub struct Algorithms {
    encryptor: Arc<str>,
    erasure: Arc<str>,
    fountain: Arc<str>,
    compression: Arc<str>,
    erasure_is_fallback: bool,
    compression_is_fallback: bool,
    erasure_emergency: bool,
    compression_emergency: bool,
}

impl Algorithms {
    fn new(
        encryptor: &str,
        erasure: &'static str,
        fountain: &'static str,
        compression: &'static str,
        erasure_emergency: bool,
        compression_emergency: bool,
    ) -> Self {
        Self {
            encryptor: Arc::<str>::from(encryptor),
            erasure: Arc::<str>::from(erasure),
            fountain: Arc::<str>::from(fountain),
            compression: Arc::<str>::from(compression),
            erasure_is_fallback: is_fallback_erasure(erasure),
            compression_is_fallback: is_fallback_compression(compression),
            erasure_emergency,
            compression_emergency,
        }
    }

    pub fn encryptor(&self) -> &str {
        &self.encryptor
    }

    pub fn erasure(&self) -> &str {
        &self.erasure
    }

    pub fn fountain(&self) -> &str {
        &self.fountain
    }

    pub fn compression(&self) -> &str {
        &self.compression
    }

    pub fn erasure_fallback(&self) -> bool {
        self.erasure_is_fallback
    }

    pub fn compression_fallback(&self) -> bool {
        self.compression_is_fallback
    }

    pub fn erasure_emergency(&self) -> bool {
        self.erasure_emergency
    }

    pub fn compression_emergency(&self) -> bool {
        self.compression_emergency
    }
}

struct Shared {
    config: Config,
    compressor: Arc<dyn Compressor>,
    erasure: Arc<dyn ErasureCoder>,
    fountain: Arc<dyn FountainCoder>,
    algorithms: Algorithms,
}

static SHARED: Lazy<RwLock<Shared>> = Lazy::new(|| {
    let default = Config::default();
    let shared = shared_from_config(default).expect("default coding config");
    RwLock::new(shared)
});

fn shared_from_config(mut config: Config) -> Result<Shared, CodingError> {
    let mut erasure_emergency = false;
    let mut compression_emergency = false;
    let rollout = config.rollout().clone();
    let emergency_env = rollout
        .emergency_switch_env
        .clone()
        .unwrap_or_else(|| DEFAULT_FALLBACK_EMERGENCY_ENV.to_string());

    if is_fallback_erasure(&config.erasure.algorithm) {
        erasure_emergency = enforce_fallback_policy(
            &rollout,
            FallbackComponent::Coder,
            &config.erasure.algorithm,
        )?;
    }
    if is_fallback_compression(&config.compression.algorithm) {
        compression_emergency = enforce_fallback_policy(
            &rollout,
            FallbackComponent::Compressor,
            &config.compression.algorithm,
        )?;
    }

    let compressor_boxed = config.compressor()?;
    let compressor_alg = compressor_boxed.algorithm();
    let compressor = Arc::<dyn Compressor>::from(compressor_boxed);

    let erasure_boxed = config.erasure_coder()?;
    let erasure_alg = erasure_boxed.algorithm();
    let erasure = Arc::<dyn ErasureCoder>::from(erasure_boxed);

    let fountain_boxed = config.fountain_coder()?;
    let fountain_alg = fountain_boxed.algorithm();
    let fountain = Arc::<dyn FountainCoder>::from(fountain_boxed);

    config.erasure.algorithm = erasure_alg.to_string();
    config.fountain.algorithm = fountain_alg.to_string();
    config.compression.algorithm = compressor_alg.to_string();

    let algorithms = Algorithms::new(
        &config.encryption.algorithm,
        erasure_alg,
        fountain_alg,
        compressor_alg,
        erasure_emergency,
        compression_emergency,
    );

    #[cfg(feature = "telemetry")]
    {
        if erasure_emergency {
            tracing::warn!(
                algorithm = %algorithms.erasure(),
                env = %emergency_env,
                "storage_erasure_fallback_emergency"
            );
        }
        if compression_emergency {
            tracing::warn!(
                algorithm = %algorithms.compression(),
                env = %emergency_env,
                "storage_compression_fallback_emergency"
            );
        }
        crate::telemetry::record_coding_algorithms(&algorithms);
    }
    #[cfg(not(feature = "telemetry"))]
    {
        if erasure_emergency {
            eprintln!(
                "storage_erasure_fallback_emergency: {} via {}",
                algorithms.erasure(),
                emergency_env
            );
        }
        if compression_emergency {
            eprintln!(
                "storage_compression_fallback_emergency: {} via {}",
                algorithms.compression(),
                emergency_env
            );
        }
    }

    Ok(Shared {
        config,
        compressor,
        erasure,
        fountain,
        algorithms,
    })
}

pub fn configure(config: Config) {
    match shared_from_config(config) {
        Ok(shared) => {
            *SHARED.write().unwrap() = shared;
        }
        Err(err) => {
            #[cfg(feature = "telemetry")]
            tracing::warn!(reason = %err, "coding_config_invalid");
            #[cfg(not(feature = "telemetry"))]
            eprintln!("coding_config_invalid: {err}");
        }
    }
}

pub fn configure_from_dir(dir: &str) {
    let path = Path::new(dir).join("storage.toml");
    match Config::load_from_path(&path) {
        Ok(config) => configure(config),
        Err(ConfigError::Io(ref err)) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            #[cfg(feature = "telemetry")]
            tracing::warn!(reason = %err, path = %path.display(), "coding_config_load_failed");
            #[cfg(not(feature = "telemetry"))]
            eprintln!(
                "coding_config_load_failed: {err} (path: {})",
                path.display()
            );
        }
    }
}

pub fn current() -> Config {
    SHARED.read().unwrap().config.clone()
}

pub fn algorithms() -> Algorithms {
    SHARED.read().unwrap().algorithms.clone()
}

pub fn compressor() -> Arc<dyn Compressor> {
    Arc::clone(&SHARED.read().unwrap().compressor)
}

pub fn compressor_for_algorithm(
    algorithm: &str,
    level: i32,
) -> Result<Arc<dyn Compressor>, CodingError> {
    let boxed = compressor_for(algorithm, level)?;
    Ok(Arc::<dyn Compressor>::from(boxed))
}

pub fn erasure() -> Arc<dyn ErasureCoder> {
    Arc::clone(&SHARED.read().unwrap().erasure)
}

pub fn erasure_for_algorithm(
    algorithm: &str,
    data_shards: usize,
    parity_shards: usize,
) -> Result<Arc<dyn ErasureCoder>, CodingError> {
    let boxed = erasure_coder_for(algorithm, data_shards, parity_shards)?;
    Ok(Arc::<dyn ErasureCoder>::from(boxed))
}

pub fn fountain() -> Arc<dyn FountainCoder> {
    Arc::clone(&SHARED.read().unwrap().fountain)
}

pub fn fountain_for_algorithm(
    algorithm: &str,
    symbol_size: u16,
    rate: f32,
) -> Result<Arc<dyn FountainCoder>, CodingError> {
    let boxed = fountain_coder_for(algorithm, symbol_size, rate)?;
    Ok(Arc::<dyn FountainCoder>::from(boxed))
}

pub fn encryptor(key: &[u8]) -> Result<Box<dyn Encryptor>, CodingError> {
    SHARED.read().unwrap().config.encryptor(key)
}

pub fn encryptor_for_algorithm(
    algorithm: &str,
    key: &[u8],
) -> Result<Box<dyn Encryptor>, CodingError> {
    encryptor_for(algorithm, key)
}

pub fn chunk_defaults() -> (usize, Vec<usize>) {
    let shared = SHARED.read().unwrap();
    let default = shared.config.chunk_default_bytes();
    let ladder = shared.config.chunk_ladder().to_vec();
    (default, ladder)
}

pub fn erasure_counts() -> (usize, usize) {
    let shared = SHARED.read().unwrap();
    (
        shared.config.erasure.data_shards,
        shared.config.erasure.parity_shards,
    )
}

pub fn compression_level() -> i32 {
    SHARED.read().unwrap().config.compression.level
}

pub fn fountain_parameters() -> (u16, f32) {
    let shared = SHARED.read().unwrap();
    (
        shared.config.fountain.symbol_size,
        shared.config.fountain.rate,
    )
}

fn is_fallback_erasure(algorithm: &str) -> bool {
    algorithm.eq_ignore_ascii_case("xor")
}

fn is_fallback_compression(algorithm: &str) -> bool {
    algorithm.eq_ignore_ascii_case("rle")
}

#[derive(Clone, Copy)]
enum FallbackComponent {
    Coder,
    Compressor,
}

fn enforce_fallback_policy(
    rollout: &coding::RolloutConfig,
    component: FallbackComponent,
    algorithm: &str,
) -> Result<bool, CodingError> {
    let emergency = emergency_switch_active(rollout);
    if emergency {
        return Ok(true);
    }

    let allowed_flag = match component {
        FallbackComponent::Coder => rollout.allow_fallback_coder,
        FallbackComponent::Compressor => rollout.allow_fallback_compressor,
    };

    if !allowed_flag || rollout.require_emergency_switch {
        return Err(CodingError::Disabled {
            algorithm: algorithm.to_string(),
        });
    }

    Ok(false)
}

fn emergency_switch_active(rollout: &coding::RolloutConfig) -> bool {
    let env_name = rollout
        .emergency_switch_env
        .as_deref()
        .unwrap_or(DEFAULT_FALLBACK_EMERGENCY_ENV);
    match env::var(env_name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

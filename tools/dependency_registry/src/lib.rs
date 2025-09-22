pub mod cli;
pub mod config;
pub mod model;
pub mod output;
pub mod registry;

pub use cli::Cli;
pub use config::PolicyConfig;
pub use model::{
    DependencyEntry, DependencyRegistry, RiskTier, ViolationEntry, ViolationKind, ViolationReport,
};
pub use registry::{build_registry, BuildOptions, BuildOutput};

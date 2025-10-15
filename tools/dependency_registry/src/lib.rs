pub mod check;
pub mod cli;
pub mod config;
pub mod model;
pub mod output;
pub mod registry;
pub mod runner;

pub use cli::Cli;
pub use config::PolicyConfig;
pub use model::{
    DependencyEntry, DependencyRegistry, RiskTier, ViolationEntry, ViolationKind, ViolationReport,
};
pub use registry::{build_registry, BuildOptions, BuildOutput};
pub use runner::{execute as run_cli, RunArtifacts};

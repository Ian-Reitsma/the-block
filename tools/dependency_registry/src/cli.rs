use std::path::PathBuf;

use clap::Parser;

/// Generate and audit the workspace dependency registry.
#[derive(Debug, Parser)]
#[command(
    name = "dependency-registry",
    version,
    about = "Workspace dependency governance registry"
)]
pub struct Cli {
    /// Path to the workspace manifest to inspect.
    #[arg(long, value_name = "PATH")]
    pub manifest_path: Option<PathBuf>,

    /// Path to the dependency policy configuration.
    #[arg(
        long,
        value_name = "PATH",
        default_value = "config/dependency_policies.toml"
    )]
    pub config: PathBuf,

    /// Optional positional override for the dependency policy configuration.
    ///
    /// This preserves backwards compatibility with tooling that passed the
    /// configuration path without `--config`.
    #[arg(value_name = "CONFIG", hide = true)]
    pub positional_config: Option<PathBuf>,

    /// Validate the freshly generated registry against the committed baseline.
    #[arg(long)]
    pub check: bool,

    /// Diff two registry snapshots instead of generating a new one.
    #[arg(long = "diff", value_names = ["OLD", "NEW"], num_args = 2)]
    pub diff: Option<Vec<PathBuf>>,

    /// Explain a crate's metadata from the existing registry snapshot.
    #[arg(long = "explain", value_name = "CRATE")]
    pub explain: Option<String>,

    /// Override the maximum permitted dependency depth.
    #[arg(long, value_name = "DEPTH")]
    pub max_depth: Option<usize>,

    /// Baseline file used for check mode and explanations.
    #[arg(
        long,
        value_name = "PATH",
        default_value = "docs/dependency_inventory.json"
    )]
    pub baseline: PathBuf,

    /// Output directory for generated artifacts.
    #[arg(long, value_name = "DIR", default_value = "target")]
    pub out_dir: PathBuf,
}

impl Cli {
    pub fn resolved_config(&self) -> PathBuf {
        self.positional_config
            .clone()
            .unwrap_or_else(|| self.config.clone())
    }
}

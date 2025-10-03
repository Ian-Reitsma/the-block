use std::path::PathBuf;

use anyhow::{anyhow, Result};
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};

/// Generate and audit the workspace dependency registry.
#[derive(Debug)]
pub struct Cli {
    /// Path to the workspace manifest to inspect.
    pub manifest_path: Option<PathBuf>,

    /// Path to the dependency policy configuration.
    pub config: PathBuf,

    /// Optional positional override for the dependency policy configuration.
    ///
    /// This preserves backwards compatibility with tooling that passed the
    /// configuration path without `--config`.
    pub positional_config: Option<PathBuf>,

    /// Validate the freshly generated registry against the committed baseline.
    pub check: bool,

    /// Diff two registry snapshots instead of generating a new one.
    pub diff: Option<Vec<PathBuf>>,

    /// Explain a crate's metadata from the existing registry snapshot.
    pub explain: Option<String>,

    /// Override the maximum permitted dependency depth.
    pub max_depth: Option<usize>,

    /// Baseline file used for check mode and explanations.
    pub baseline: PathBuf,

    /// Output directory for generated artifacts.
    pub out_dir: PathBuf,

    /// Optional path to emit a frozen dependency snapshot for releases.
    pub snapshot: Option<PathBuf>,
}

impl Cli {
    pub fn build_command() -> Command {
        CommandBuilder::new(
            CommandId("dependency-registry"),
            "dependency-registry",
            "Workspace dependency governance registry",
        )
        .arg(ArgSpec::Option(OptionSpec::new(
            "manifest-path",
            "manifest-path",
            "Path to the workspace manifest to inspect",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("config", "config", "Dependency policy configuration path")
                .default("config/dependency_policies.toml"),
        ))
        .arg(ArgSpec::Positional(
            PositionalSpec::new("config", "Configuration path override").optional(),
        ))
        .arg(ArgSpec::Flag(FlagSpec::new(
            "check",
            "check",
            "Validate the generated registry against the committed baseline",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("diff", "diff", "Diff two registry snapshots").value_arity(2),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "explain",
            "explain",
            "Explain a crate from the baseline registry",
        )))
        .arg(ArgSpec::Option(OptionSpec::new(
            "max-depth",
            "max-depth",
            "Override the maximum dependency depth",
        )))
        .arg(ArgSpec::Option(
            OptionSpec::new("baseline", "baseline", "Baseline registry snapshot")
                .default("docs/dependency_inventory.json"),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("out-dir", "out-dir", "Directory for generated artifacts")
                .default("target"),
        ))
        .arg(ArgSpec::Option(OptionSpec::new(
            "snapshot",
            "snapshot",
            "Emit a frozen dependency snapshot",
        )))
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self> {
        let manifest_path = matches.get_string("manifest-path").map(PathBuf::from);
        let config = matches
            .get_string("config")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("config/dependency_policies.toml"));
        let positional_config = matches
            .get_positional("config")
            .and_then(|values| values.first().cloned())
            .map(PathBuf::from);
        let check = matches.get_flag("check");
        let diff = {
            let values = matches.get_strings("diff");
            if values.is_empty() {
                None
            } else if values.len() == 2 {
                Some(values.into_iter().map(PathBuf::from).collect())
            } else {
                return Err(anyhow!(
                    "--diff expects exactly two paths (old and new snapshots)"
                ));
            }
        };
        let explain = matches.get_string("explain");
        let max_depth = matches
            .get("max-depth")
            .map(|value| value.parse::<usize>().map_err(|err| anyhow!(err)))
            .transpose()?;
        let baseline = matches
            .get_string("baseline")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("docs/dependency_inventory.json"));
        let out_dir = matches
            .get_string("out-dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target"));
        let snapshot = matches.get_string("snapshot").map(PathBuf::from);

        Ok(Self {
            manifest_path,
            config,
            positional_config,
            check,
            diff,
            explain,
            max_depth,
            baseline,
            out_dir,
            snapshot,
        })
    }

    pub fn resolved_config(&self) -> PathBuf {
        self.positional_config
            .clone()
            .unwrap_or_else(|| self.config.clone())
    }
}

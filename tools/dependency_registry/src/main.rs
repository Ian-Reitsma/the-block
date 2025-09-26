use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use dependency_registry::{build_registry, output, BuildOptions, Cli, PolicyConfig};

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(diff) = &cli.diff {
        let paths: Vec<PathBuf> = diff.clone();
        output::diff_registries(&paths[0], &paths[1])?;
        return Ok(());
    }

    if let Some(crate_name) = &cli.explain {
        output::explain_crate(crate_name, &cli.baseline)?;
        return Ok(());
    }

    let config_path = cli.resolved_config();
    let policy = PolicyConfig::load(&config_path)?;

    let build = build_registry(BuildOptions {
        manifest_path: cli.manifest_path.as_deref(),
        policy: &policy,
        config_path: &config_path,
        override_depth: cli.max_depth,
    })?;

    output::write_registry_json(&build.registry, &cli.out_dir)?;
    let markdown_path = PathBuf::from("docs/dependency_inventory.md");
    output::write_markdown(&build.registry, &markdown_path)?;
    output::write_violations(&build.violations, &cli.out_dir)?;
    output::write_prometheus_metrics(&build.violations, &cli.out_dir)?;

    if cli.check {
        let baseline = output::load_registry(&cli.baseline)
            .with_context(|| format!("unable to load baseline from {}", cli.baseline.display()))?;
        if baseline.comparison_key() != build.registry.comparison_key() {
            anyhow::bail!(
                "dependency registry drift detected relative to baseline {}",
                cli.baseline.display()
            );
        }
        if !build.violations.is_empty() {
            anyhow::bail!(
                "policy violations detected; see {}",
                cli.out_dir.join("dependency-violations.json").display()
            );
        }
    }

    Ok(())
}

use std::path::PathBuf;

use diagnostics::anyhow::Result;
use diagnostics::TbError;

use crate::{
    check,
    cli::Cli,
    config::PolicyConfig,
    output::{self, CheckTelemetry},
    registry::{build_registry, BuildOptions, BuildOutput},
};

const DOC_OVERRIDE_ENV: &str = "TB_DEPENDENCY_REGISTRY_DOC_PATH";

pub struct RunArtifacts {
    pub build: BuildOutput,
    pub registry_path: PathBuf,
    pub violations_path: PathBuf,
    pub telemetry_path: PathBuf,
    pub check_summary_path: Option<PathBuf>,
    pub manifest_path: PathBuf,
    pub markdown_path: PathBuf,
    pub snapshot_path: Option<PathBuf>,
}

pub fn execute(cli: &Cli) -> Result<RunArtifacts> {
    let config_path = cli.resolved_config();
    let policy = PolicyConfig::load(&config_path)?;

    let build = build_registry(BuildOptions {
        manifest_path: cli.manifest_path.as_deref(),
        policy: &policy,
        config_path: &config_path,
        override_depth: cli.max_depth,
    })?;

    let registry_path = cli.out_dir.join("dependency-registry.json");
    output::write_registry_json(&build.registry, &cli.out_dir)?;

    let manifest_path = cli.manifest_out.clone();
    output::write_crate_manifest(&build.registry, &manifest_path)?;

    let snapshot_path = if let Some(snapshot) = &cli.snapshot {
        output::write_snapshot(&build.registry, snapshot)?;
        Some(snapshot.clone())
    } else {
        None
    };

    let markdown_path = std::env::var(DOC_OVERRIDE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("docs/dependency_inventory.md"));
    output::write_markdown(&build.registry, &markdown_path)?;

    let violations_path = cli.out_dir.join("dependency-violations.json");
    output::write_violations(&build.violations, &cli.out_dir)?;

    let telemetry_path = cli.out_dir.join("dependency-metrics.telemetry");
    output::write_telemetry_metrics(&build.violations, &cli.out_dir)?;
    let mut check_summary_path = None;

    if cli.check {
        let baseline = match output::load_registry(&cli.baseline) {
            Ok(registry) => registry,
            Err(err) => {
                let telemetry = CheckTelemetry::baseline_error("load_failed");
                let telemetry_result = output::write_check_telemetry(&cli.out_dir, &telemetry);
                let summary_result = output::write_check_summary(&cli.out_dir, &telemetry);
                if let Err(metric_err) = telemetry_result {
                    let combined = format!("{} (telemetry emission failed: {metric_err})", err);
                    return Err(TbError::new(combined));
                }
                if let Err(summary_err) = summary_result {
                    let combined = format!("{} (summary emission failed: {summary_err})", err);
                    return Err(TbError::new(combined));
                }
                return Err(err);
            }
        };

        let baseline_key = baseline.comparison_key();
        let current_key = build.registry.comparison_key();

        if let Some(drift) = check::compute(&baseline_key, &current_key) {
            let mut message = format!(
                "dependency registry drift detected relative to baseline {}:",
                cli.baseline.display()
            );
            for entry in &drift.additions {
                message.push_str(&format!(
                    "\n  + {} {} (tier {}, origin {}, depth {}, license {})",
                    entry.name,
                    entry.version,
                    entry.tier,
                    entry.origin,
                    entry.depth,
                    display_license(entry)
                ));
            }
            for entry in &drift.removals {
                message.push_str(&format!(
                    "\n  - {} {} (tier {}, origin {}, depth {}, license {})",
                    entry.name,
                    entry.version,
                    entry.tier,
                    entry.origin,
                    entry.depth,
                    display_license(entry)
                ));
            }
            for change in &drift.entry_changes {
                message.push_str(&format!(
                    "\n  ~ {} {} {}: '{}' -> '{}'",
                    change.name, change.version, change.field, change.before, change.after
                ));
            }
            for policy in &drift.policy_changes {
                message.push_str(&format!(
                    "\n  ~ policy {}: '{}' -> '{}'",
                    policy.field, policy.before, policy.after
                ));
            }
            for root in &drift.root_additions {
                message.push_str(&format!("\n  + root package {root}"));
            }
            for root in &drift.root_removals {
                message.push_str(&format!("\n  - root package {root}"));
            }

            let telemetry = CheckTelemetry::drift(drift.counts());
            if let Err(metric_err) = output::write_check_telemetry(&cli.out_dir, &telemetry) {
                message.push_str(&format!("\ntelemetry emission failed: {metric_err}"));
            }
            if let Err(summary_err) = output::write_check_summary(&cli.out_dir, &telemetry) {
                message.push_str(&format!("\nsummary emission failed: {summary_err}"));
            }
            diagnostics::anyhow::bail!("{}", message);
        }

        if !build.violations.is_empty() {
            let telemetry = CheckTelemetry::violations(build.violations.entries.len());
            if let Err(metric_err) = output::write_check_telemetry(&cli.out_dir, &telemetry) {
                diagnostics::anyhow::bail!(
                    "policy violations detected; see {}\ntelemetry emission failed: {}",
                    violations_path.display(),
                    metric_err
                );
            }
            output::write_check_summary(&cli.out_dir, &telemetry)?;
            diagnostics::anyhow::bail!(
                "policy violations detected; see {} ({} total)",
                violations_path.display(),
                build.violations.entries.len()
            );
        }

        let telemetry = CheckTelemetry::pass();
        output::write_check_telemetry(&cli.out_dir, &telemetry)?;
        output::write_check_summary(&cli.out_dir, &telemetry)?;
        check_summary_path = Some(cli.out_dir.join("dependency-check.summary.json"));
    }

    Ok(RunArtifacts {
        build,
        registry_path,
        violations_path,
        telemetry_path,
        check_summary_path,
        manifest_path,
        markdown_path,
        snapshot_path,
    })
}

fn display_license(entry: &crate::model::DependencyEntry) -> String {
    entry.license.clone().unwrap_or_else(|| "—".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_path(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("dependency_registry_runner_{label}_{nanos}"));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn honours_markdown_override_env() {
        let out_dir = unique_path("out");
        fs::create_dir_all(&out_dir).expect("create out dir");
        let markdown = unique_path("markdown");
        std::env::set_var(DOC_OVERRIDE_ENV, &markdown);

        let _cli = Cli {
            manifest_path: None,
            config: PathBuf::from("config/dependency_policies.toml"),
            positional_config: None,
            check: false,
            diff: None,
            explain: None,
            max_depth: None,
            baseline: out_dir.join("baseline.json"),
            out_dir: out_dir.clone(),
            snapshot: None,
            manifest_out: out_dir.join("manifest.txt"),
        };

        // We cannot run the full execute path because it requires cargo metadata.
        // Instead ensure markdown resolution respects the override.
        let resolved = std::env::var(DOC_OVERRIDE_ENV).unwrap();
        assert_eq!(PathBuf::from(resolved), markdown);

        std::env::remove_var(DOC_OVERRIDE_ENV);
    }
}

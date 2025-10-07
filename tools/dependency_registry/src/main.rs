use std::path::PathBuf;

use cli_core::{
    command::Command,
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use diagnostics::anyhow as diag_anyhow;
use diagnostics::anyhow::{Context, Result};
use std::env;

use dependency_registry::{build_registry, output, BuildOptions, Cli, PolicyConfig};

fn main() -> Result<()> {
    let mut argv = env::args();
    let bin = argv
        .next()
        .unwrap_or_else(|| "dependency-registry".to_string());
    let args: Vec<String> = argv.collect();

    let command = Cli::build_command();
    if args.is_empty() {
        print_root_help(&command, &bin);
        return Ok(());
    }

    let parser = Parser::new(&command);
    let matches = match parser.parse(&args) {
        Ok(matches) => matches,
        Err(ParseError::HelpRequested(path)) => {
            print_help_for_path(&command, &path);
            return Ok(());
        }
        Err(err) => return Err(diag_anyhow::anyhow!(err)),
    };

    let cli = Cli::from_matches(&matches)?;

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
    output::write_crate_manifest(&build.registry, &cli.manifest_out)?;
    if let Some(snapshot_path) = &cli.snapshot {
        output::write_snapshot(&build.registry, snapshot_path)?;
    }
    let markdown_path = PathBuf::from("docs/dependency_inventory.md");
    output::write_markdown(&build.registry, &markdown_path)?;
    output::write_violations(&build.violations, &cli.out_dir)?;
    output::write_prometheus_metrics(&build.violations, &cli.out_dir)?;

    if cli.check {
        let baseline = output::load_registry(&cli.baseline)
            .with_context(|| format!("unable to load baseline from {}", cli.baseline.display()))?;
        if baseline.comparison_key() != build.registry.comparison_key() {
            diag_anyhow::bail!(
                "dependency registry drift detected relative to baseline {}",
                cli.baseline.display()
            );
        }
        if !build.violations.is_empty() {
            diag_anyhow::bail!(
                "policy violations detected; see {}",
                cli.out_dir.join("dependency-violations.json").display()
            );
        }
    }

    Ok(())
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} --help' or '{bin} help <section>' for more information.");
}

fn print_help_for_path(root: &Command, path: &str) {
    let segments: Vec<&str> = path.split_whitespace().collect();
    if let Some(cmd) = find_command(root, &segments) {
        let generator = HelpGenerator::new(cmd);
        println!("{}", generator.render());
    }
}

fn find_command<'a>(root: &'a Command, path: &[&str]) -> Option<&'a Command> {
    if path.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for segment in path.iter().skip(1) {
        if let Some(next) = current
            .subcommands
            .iter()
            .find(|command| command.name == *segment)
        {
            current = next;
        } else {
            return None;
        }
    }
    Some(current)
}

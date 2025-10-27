use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    help::HelpGenerator,
    parse::{ParseError, Parser},
};
use diagnostics::{anyhow, bail, Context, Result, TbError};
extern crate foundation_serialization as serde;

use foundation_serialization::{json, Deserialize};
use monitoring_build::ChaosReadinessSnapshot;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command as StdCommand,
};

#[derive(Default)]
struct Summary {
    balance_changed: bool,
    pending_changed: bool,
    title_ok: bool,
}

impl Summary {
    fn to_json_value(&self) -> json::Value {
        let mut map = json::Map::new();
        map.insert(
            "balance_changed".to_string(),
            json::Value::Bool(self.balance_changed),
        );
        map.insert(
            "pending_changed".to_string(),
            json::Value::Bool(self.pending_changed),
        );
        map.insert("title_ok".to_string(), json::Value::Bool(self.title_ok));
        json::Value::Object(map)
    }

    fn write_pretty_json(&self, mut writer: impl Write) -> Result<()> {
        let value = self.to_json_value();
        json::to_writer_pretty(&mut writer, &value)
            .map_err(|err| anyhow!(err))
            .context("failed to write summary output")
    }
}

const OVERLAY_EPSILON: f64 = 1e-6;

#[derive(Deserialize)]
struct OverlayReadinessRow {
    scenario: String,
    module: String,
    site: String,
    provider: String,
    readiness: f64,
    scenario_readiness: f64,
    readiness_before: Option<f64>,
    provider_before: Option<String>,
}

#[derive(Deserialize)]
struct StatusDiffEntry {
    scenario: String,
    module: String,
    #[serde(default)]
    readiness_before: Option<f64>,
    #[serde(default)]
    readiness_after: Option<f64>,
    #[serde(default)]
    site_added: Vec<DiffSite>,
    #[serde(default)]
    site_removed: Vec<DiffSite>,
    #[serde(default)]
    site_changed: Vec<DiffChange>,
}

#[derive(Deserialize)]
struct DiffSite {
    site: String,
    #[allow(dead_code)]
    #[serde(default)]
    provider_kind: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct DiffChange {
    site: String,
    #[serde(default)]
    before: Option<f64>,
    #[serde(default)]
    after: Option<f64>,
    #[serde(default)]
    provider_before: Option<String>,
    #[serde(default)]
    provider_after: Option<String>,
}

#[derive(Deserialize)]
struct ProviderScenarioReport {
    scenario: String,
    module: String,
    #[serde(default = "usize::default")]
    impacted_sites: usize,
    #[serde(default)]
    readiness_before: f64,
    #[serde(default)]
    readiness_after: f64,
    #[serde(default = "usize::default")]
    diff_entries: usize,
}

#[derive(Deserialize)]
struct ProviderFailoverReport {
    provider: String,
    #[serde(default = "usize::default")]
    total_diff_entries: usize,
    #[serde(default)]
    scenarios: Vec<ProviderScenarioReport>,
}

fn main() -> Result<()> {
    let mut argv = std::env::args();
    let bin = argv.next().unwrap_or_else(|| "xtask".to_string());
    let args: Vec<String> = argv.collect();

    let command = build_command();
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
        Err(err) => return Err(TbError::from_error(err)),
    };

    match matches.subcommand() {
        Some(("summary", sub_matches)) => run_summary(sub_matches),
        Some(("check-deps", sub_matches)) => run_check_deps(sub_matches),
        Some(("chaos", sub_matches)) => run_chaos(sub_matches),
        None => {
            print_root_help(&command, &bin);
            Ok(())
        }
        Some((other, _)) => bail!("unknown subcommand: {other}"),
    }
}

fn build_command() -> Command {
    CommandBuilder::new(CommandId("xtask"), "xtask", "Repository automation tasks")
        .subcommand(
            CommandBuilder::new(
                CommandId("xtask.summary"),
                "summary",
                "Summarise diff state",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("base", "base", "Base branch to diff against")
                    .default("origin/main"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "title",
                "title",
                "Proposed PR title",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("xtask.check_deps"),
                "check-deps",
                "Run the dependency registry check with first-party guard outputs",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "manifest-out",
                    "manifest-out",
                    "Path to write the crate manifest used by build guards",
                )
                .default("config/first_party_manifest.txt"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "out-dir",
                    "out-dir",
                    "Output directory for registry artefacts",
                )
                .default("target/dependency-registry"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "config",
                "config",
                "Dependency policy configuration path override",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "baseline",
                "baseline",
                "Baseline registry snapshot used for drift detection",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("xtask.chaos"),
                "chaos",
                "Run the WAN chaos verifier suite",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("out-dir", "out-dir", "Output directory for chaos artefacts")
                    .default("target/chaos"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("steps", "steps", "Simulation steps to execute").default("120"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("nodes", "nodes", "Number of simulated nodes").default("256"),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "status-endpoint",
                "status-endpoint",
                "Fetch chaos/status baseline from the metrics aggregator endpoint",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "baseline",
                "baseline",
                "Baseline snapshot path used for diffs",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "require-diff",
                "require-diff",
                "Fail when chaos/status diff is empty",
            )))
            .build(),
        )
        .build()
}

fn print_root_help(command: &Command, bin: &str) {
    let generator = HelpGenerator::new(command);
    println!("{}", generator.render());
    println!("\nRun '{bin} --help' for more details.");
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

fn run_summary(matches: &cli_core::parse::Matches) -> Result<()> {
    let base_ref = matches
        .get_string("base")
        .unwrap_or_else(|| "origin/main".to_string());
    let title = matches.get_string("title");

    let mut summary = Summary::default();
    let diff_output = git_diff(&base_ref)?;
    for line in diff_output.lines() {
        if line.contains("balance") {
            summary.balance_changed = true;
        }
        if line.contains("pending_") {
            summary.pending_changed = true;
        }
        if summary.balance_changed && summary.pending_changed {
            break;
        }
    }

    if let Some(t) = title {
        if summary.balance_changed || summary.pending_changed {
            summary.title_ok = t.starts_with("[core]");
        } else {
            summary.title_ok = true;
        }
    }

    summary.write_pretty_json(io::stdout())?;
    if !summary.title_ok {
        bail!("PR title does not match modified areas");
    }
    Ok(())
}

fn run_chaos(matches: &cli_core::parse::Matches) -> Result<()> {
    let out_dir = matches
        .get_string("out-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/chaos"));
    fs::create_dir_all(&out_dir).context("failed to create chaos output directory")?;

    let steps = matches
        .get_string("steps")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120);
    let nodes = matches
        .get_string("nodes")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(256);
    let status_endpoint = matches.get_string("status-endpoint");
    let require_diff = matches.get_flag("require-diff");
    let baseline_path = matches
        .get_string("baseline")
        .map(PathBuf::from)
        .unwrap_or_else(|| out_dir.join("status.baseline.json"));

    let attestation_path = out_dir.join("attestations.json");
    let snapshot_path = out_dir.join("status.snapshot.json");
    let diff_path = out_dir.join("status.diff.json");
    let overlay_path = out_dir.join("overlay.readiness.json");
    let provider_failover_path = out_dir.join("provider.failover.json");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = StdCommand::new(cargo);
    command
        .args(["run", "-p", "tb-sim", "--bin", "chaos_lab", "--quiet"])
        .env("TB_CHAOS_ATTESTATIONS", &attestation_path)
        .env("TB_CHAOS_STATUS_SNAPSHOT", &snapshot_path)
        .env("TB_CHAOS_STATUS_DIFF", &diff_path)
        .env("TB_CHAOS_OVERLAY_READINESS", &overlay_path)
        .env("TB_CHAOS_PROVIDER_FAILOVER", &provider_failover_path)
        .env("TB_CHAOS_STEPS", steps.to_string())
        .env("TB_CHAOS_NODE_COUNT", nodes.to_string());

    match status_endpoint.as_ref() {
        Some(endpoint) => {
            command.env("TB_CHAOS_STATUS_ENDPOINT", endpoint);
            command.env("TB_CHAOS_STATUS_BASELINE", &baseline_path);
        }
        None => {
            if baseline_path.exists() {
                command.env("TB_CHAOS_STATUS_BASELINE", &baseline_path);
            }
        }
    }

    if require_diff {
        command.env("TB_CHAOS_REQUIRE_DIFF", "1");
    }

    let status = command
        .status()
        .context("failed to execute chaos verifier")?;
    if !status.success() {
        bail!("chaos verifier failed with status {status}");
    }

    let snapshots = load_snapshots(&snapshot_path)?;
    let overlay_rows = load_overlay_rows(&overlay_path)?;
    let mut modules: BTreeMap<String, usize> = BTreeMap::new();
    for snapshot in &snapshots {
        let key = snapshot.module.as_str().to_string();
        *modules.entry(key).or_insert(0) += 1;
    }

    println!("chaos snapshots captured:");
    for (module, count) in modules {
        println!("  {:<8} {count}", module);
    }

    let providers: BTreeSet<String> = overlay_rows
        .iter()
        .map(|row| row.provider.clone())
        .collect();
    println!(
        "overlay readiness rows: {} (providers: {})",
        overlay_rows.len(),
        providers.len()
    );
    if overlay_rows.iter().any(|row| row.provider.is_empty()) {
        bail!("overlay readiness rows missing provider labels");
    }
    let mut overlay_scenarios: BTreeMap<String, usize> = BTreeMap::new();
    let mut overlay_modules: BTreeMap<String, usize> = BTreeMap::new();
    let mut scenario_readiness: BTreeMap<String, f64> = BTreeMap::new();
    let mut readiness_improvements = 0usize;
    let mut readiness_regressions = 0usize;
    let mut provider_changes = 0usize;
    let mut duplicate_sites = 0usize;
    let mut unique_sites: BTreeSet<(String, String)> = BTreeSet::new();
    for row in &overlay_rows {
        *overlay_scenarios.entry(row.scenario.clone()).or_insert(0) += 1;
        *overlay_modules.entry(row.module.clone()).or_insert(0) += 1;
        scenario_readiness.insert(row.scenario.clone(), row.scenario_readiness);
        if !unique_sites.insert((row.scenario.clone(), row.site.clone())) {
            duplicate_sites += 1;
        }
        if let Some(before) = row.readiness_before {
            if row.readiness + OVERLAY_EPSILON < before {
                readiness_regressions += 1;
            } else if row.readiness > before + OVERLAY_EPSILON {
                readiness_improvements += 1;
            }
        }
        if let Some(previous) = &row.provider_before {
            if previous != &row.provider {
                provider_changes += 1;
            }
        }
    }
    if !overlay_modules.is_empty() {
        println!("    modules:");
        for (module, count) in &overlay_modules {
            println!("      {module:<16} rows={count}");
        }
    }
    println!(
        "    readiness deltas: improvements={} regressions={} provider-changes={}",
        readiness_improvements, readiness_regressions, provider_changes
    );
    if duplicate_sites > 0 {
        println!(
            "    duplicate site entries detected: {} (scenario,site pairs)",
            duplicate_sites
        );
    } else {
        println!(
            "    duplicate site entries detected: 0 ({} unique sites)",
            unique_sites.len()
        );
    }
    println!("    scenarios:");
    for (scenario, count) in &overlay_scenarios {
        let readiness = scenario_readiness
            .get(scenario)
            .copied()
            .unwrap_or_default();
        println!("      {scenario:<20} rows={count:<3} readiness={readiness:.3}");
    }

    let diff_entries = load_status_diff(&diff_path)?;
    if diff_entries.is_empty() {
        println!("chaos/status diff entries: 0");
    } else {
        println!("chaos/status diff entries: {}", diff_entries.len());
        for entry in diff_entries
            .iter()
            .filter(|entry| entry.module == "overlay")
        {
            let before = entry.readiness_before.unwrap_or_default();
            let after = entry.readiness_after.unwrap_or_default();
            println!(
                "  overlay {:<20} readiness {:.3} -> {:.3} added={} removed={} changed={}",
                entry.scenario,
                before,
                after,
                entry.site_added.len(),
                entry.site_removed.len(),
                entry.site_changed.len()
            );
        }
    }

    let mut overlay_regressions = Vec::new();
    for entry in &diff_entries {
        if entry.module != "overlay" {
            continue;
        }
        if let (Some(before), Some(after)) = (entry.readiness_before, entry.readiness_after) {
            if after + OVERLAY_EPSILON < before {
                overlay_regressions.push(format!(
                    "scenario '{}' readiness dropped from {:.3} to {:.3}",
                    entry.scenario, before, after
                ));
            }
        }
        if !entry.site_removed.is_empty() {
            let sites: Vec<_> = entry
                .site_removed
                .iter()
                .map(|site| site.site.as_str())
                .collect();
            overlay_regressions.push(format!(
                "scenario '{}' lost provider sites: {}",
                entry.scenario,
                sites.join(", ")
            ));
        }
    }
    if !overlay_regressions.is_empty() {
        for regression in &overlay_regressions {
            println!("!! overlay regression: {regression}");
        }
        bail!("{}", overlay_regressions.join("; "));
    }

    let provider_reports = load_provider_reports(&provider_failover_path)?;
    if provider_reports.is_empty() {
        println!("provider failover drills: none");
    } else {
        println!("provider failover drills:");
        for report in &provider_reports {
            if report.scenarios.is_empty() {
                println!("  {:<12} skipped (no overlay sites)", report.provider);
                continue;
            }
            println!(
                "  {:<12} scenarios={} diff_entries={}",
                report.provider,
                report.scenarios.len(),
                report.total_diff_entries
            );
            for scenario in &report.scenarios {
                println!(
                    "    {:<20} sites={:<2} readiness {:.3} -> {:.3} diff={}",
                    scenario.scenario,
                    scenario.impacted_sites,
                    scenario.readiness_before,
                    scenario.readiness_after,
                    scenario.diff_entries
                );
            }
        }
    }

    let mut provider_failures = Vec::new();
    for report in &provider_reports {
        if report.scenarios.is_empty() {
            continue;
        }
        if report.total_diff_entries == 0 {
            provider_failures.push(format!(
                "provider '{}' failover produced no diff entries",
                report.provider
            ));
        }
        for scenario in &report.scenarios {
            if scenario.module != "overlay" {
                continue;
            }
            if scenario.diff_entries == 0 {
                provider_failures.push(format!(
                    "provider '{}' scenario '{}' reported zero diff entries",
                    report.provider, scenario.scenario
                ));
            }
            if !(scenario.readiness_after + OVERLAY_EPSILON < scenario.readiness_before) {
                provider_failures.push(format!(
                    "provider '{}' scenario '{}' readiness did not drop (before {:.3} after {:.3})",
                    report.provider,
                    scenario.scenario,
                    scenario.readiness_before,
                    scenario.readiness_after
                ));
            }
        }
    }
    if !provider_failures.is_empty() {
        for failure in &provider_failures {
            println!("!! provider failover gating: {failure}");
        }
        bail!("{}", provider_failures.join("; "));
    }

    if let Some(ref endpoint) = status_endpoint {
        println!("fetched chaos/status baseline from {endpoint}");
    } else if baseline_path.exists() {
        println!(
            "used existing chaos/status baseline at {}",
            baseline_path.display()
        );
    }

    Ok(())
}

fn load_snapshots(path: &Path) -> Result<Vec<ChaosReadinessSnapshot>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let snapshots =
        json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(snapshots)
}

fn load_overlay_rows(path: &Path) -> Result<Vec<OverlayReadinessRow>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let rows =
        json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(rows)
}

fn load_status_diff(path: &Path) -> Result<Vec<StatusDiffEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let entries =
        json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(entries)
}

fn load_provider_reports(path: &Path) -> Result<Vec<ProviderFailoverReport>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let reports =
        json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(reports)
}

fn git_diff(base_ref: &str) -> Result<String> {
    let status = StdCommand::new("git")
        .args(["rev-parse", &format!("{base_ref}^{{commit}}")])
        .status()
        .context("failed to resolve base revision")?;
    if !status.success() {
        bail!("failed to resolve base revision: {base_ref}");
    }

    let diff_range = format!("{base_ref}..HEAD");
    let output = StdCommand::new("git")
        .args(["diff", "--patch", &diff_range])
        .output()
        .context("failed to execute git diff")?;
    if !output.status.success() {
        bail!("git diff exited with status {}", output.status);
    }
    String::from_utf8(output.stdout).context("git diff output was not valid UTF-8")
}

fn run_check_deps(matches: &cli_core::parse::Matches) -> Result<()> {
    let manifest_path = matches
        .get_string("manifest-out")
        .unwrap_or_else(|| "config/first_party_manifest.txt".to_string());
    let out_dir = matches
        .get_string("out-dir")
        .unwrap_or_else(|| "target/dependency-registry".to_string());
    let config = matches.get_string("config");
    let baseline = matches.get_string("baseline");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = StdCommand::new(cargo);
    command
        .arg("run")
        .arg("-p")
        .arg("dependency_registry")
        .arg("--");
    command
        .arg("--manifest-out")
        .arg(&manifest_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--check");

    if let Some(config_path) = config {
        command.arg("--config").arg(config_path);
    }
    if let Some(baseline_path) = baseline {
        command.arg("--baseline").arg(baseline_path);
    }

    let status = command
        .status()
        .context("failed to execute dependency-registry")?;
    if !status.success() {
        bail!("dependency registry check failed with status {status}");
    }

    let out_dir_path = Path::new(&out_dir);
    let summary_path = out_dir_path.join("dependency-check.summary.json");
    let summary_bytes = fs::read(&summary_path)
        .with_context(|| format!("failed to read {}", summary_path.display()))?;
    let summary_value = json::value_from_slice(&summary_bytes)
        .context("failed to parse dependency-check.summary.json")?;
    let status_label = summary_value
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let detail = summary_value
        .get("detail")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    println!("dependency registry check status: {status_label} ({detail})",);

    if let Some(counts) = summary_value
        .get("counts")
        .and_then(|value| value.as_object())
    {
        if !counts.is_empty() {
            let mut kinds: Vec<_> = counts.keys().cloned().collect();
            kinds.sort();
            println!("dependency registry drift counters:");
            for kind in kinds {
                let rendered = counts
                    .get(&kind)
                    .and_then(|value| value.as_i64().map(|v| v.to_string()))
                    .or_else(|| {
                        counts
                            .get(&kind)
                            .and_then(|value| value.as_u64().map(|v| v.to_string()))
                    })
                    .unwrap_or_else(|| counts.get(&kind).unwrap().to_string());
                println!("  {kind}: {rendered}");
            }
        }
    }
    Ok(())
}

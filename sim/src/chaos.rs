#![forbid(unsafe_code)]

use crate::Simulation;
use diagnostics::tracing::{debug, warn};
use monitoring_build::{ChaosAttestationDraft, ChaosSiteReadiness};
pub use monitoring_build::{ChaosModule, ChaosProviderKind};
use rand::{rngs::StdRng, Rng};
use std::collections::HashMap;

const MAX_FAULT_SEVERITY: f64 = 0.85;

#[derive(Clone, Debug)]
pub struct ChaosScenario {
    pub name: String,
    pub module: ChaosModule,
    pub sla_threshold: f64,
    pub events: Vec<ChaosEvent>,
    pub recovery_rate: f64,
    pub sites: Vec<ChaosSite>,
}

impl ChaosScenario {
    pub fn new(
        name: impl Into<String>,
        module: ChaosModule,
        sla_threshold: f64,
        recovery_rate: f64,
    ) -> Self {
        Self {
            name: name.into(),
            module,
            sla_threshold: sla_threshold.clamp(0.0, 1.0),
            events: Vec::new(),
            recovery_rate: recovery_rate.clamp(0.0, 1.0),
            sites: Vec::new(),
        }
    }

    pub fn add_event(mut self, event: ChaosEvent) -> Self {
        self.events.push(event);
        self
    }

    pub fn add_site(mut self, site: ChaosSite) -> Self {
        self.sites.push(site);
        self
    }

    pub fn set_sites(&mut self, sites: Vec<ChaosSite>) {
        self.sites = sites;
    }
}

#[derive(Clone, Debug)]
pub struct ChaosEvent {
    pub start_step: u64,
    pub duration: u64,
    pub target_fraction: f64,
    pub fault: ChaosFault,
}

impl ChaosEvent {
    pub fn new(start_step: u64, duration: u64, target_fraction: f64, fault: ChaosFault) -> Self {
        Self {
            start_step,
            duration,
            target_fraction: target_fraction.clamp(0.0, 1.0),
            fault,
        }
    }

    pub fn impact(&self) -> f64 {
        (self.target_fraction * self.fault.severity()).min(MAX_FAULT_SEVERITY)
    }

    pub fn is_active(&self, step: u64) -> bool {
        step >= self.start_step && step < self.start_step + self.duration
    }
}

#[derive(Clone, Debug)]
pub enum ChaosFault {
    OverlayPartition { loss_ratio: f64 },
    StorageCorruption { shard_loss: f64 },
    ComputeBackpressure { throttle_ratio: f64 },
}

#[derive(Clone, Debug)]
pub struct ChaosSite {
    pub name: String,
    pub weight: f64,
    pub latency_penalty: f64,
    pub provider_kind: ChaosProviderKind,
}

impl ChaosSite {
    pub fn new(name: impl Into<String>, weight: f64, latency_penalty: f64) -> Self {
        Self::with_kind(name, weight, latency_penalty, ChaosProviderKind::Unknown)
    }

    pub fn with_kind(
        name: impl Into<String>,
        weight: f64,
        latency_penalty: f64,
        provider_kind: ChaosProviderKind,
    ) -> Self {
        Self {
            name: name.into(),
            weight: weight.max(0.0),
            latency_penalty: latency_penalty.clamp(0.0, 1.0),
            provider_kind,
        }
    }
}

impl ChaosFault {
    fn severity(&self) -> f64 {
        match self {
            ChaosFault::OverlayPartition { loss_ratio } => loss_ratio.clamp(0.0, 1.0),
            ChaosFault::StorageCorruption { shard_loss } => shard_loss.clamp(0.0, 1.0),
            ChaosFault::ComputeBackpressure { throttle_ratio } => throttle_ratio.clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SiteReadinessState {
    readiness: f64,
    provider_kind: ChaosProviderKind,
}

impl SiteReadinessState {
    fn new(provider_kind: ChaosProviderKind) -> Self {
        Self {
            readiness: 1.0,
            provider_kind,
        }
    }

    pub fn readiness(&self) -> f64 {
        self.readiness
    }

    pub fn provider_kind(&self) -> ChaosProviderKind {
        self.provider_kind
    }

    pub(crate) fn set_provider_kind(&mut self, provider_kind: ChaosProviderKind) {
        self.provider_kind = provider_kind;
    }

    pub(crate) fn set_readiness(&mut self, readiness: f64) {
        self.readiness = readiness;
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChaosModuleState {
    pub readiness: f64,
    pub sla_threshold: f64,
    pub breaches: u64,
    pub window_start: u64,
    pub window_end: u64,
    pub last_step: u64,
    pub scenario: String,
    pub site_readiness: HashMap<String, SiteReadinessState>,
}

impl ChaosModuleState {
    fn new(threshold: f64, scenario: String) -> Self {
        Self {
            readiness: 1.0,
            sla_threshold: threshold,
            breaches: 0,
            window_start: 0,
            window_end: 0,
            last_step: 0,
            scenario,
            site_readiness: HashMap::new(),
        }
    }

    fn update_window(&mut self, step: u64) {
        if self.window_start == 0 {
            self.window_start = step;
        }
        self.window_end = step;
    }

    fn register_breach(&mut self) {
        self.breaches = self.breaches.saturating_add(1);
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChaosReport {
    pub overlay: ChaosModuleState,
    pub storage: ChaosModuleState,
    pub compute: ChaosModuleState,
}

impl ChaosReport {
    pub fn as_map(&self) -> HashMap<ChaosModule, &ChaosModuleState> {
        let mut map = HashMap::new();
        map.insert(ChaosModule::Overlay, &self.overlay);
        map.insert(ChaosModule::Storage, &self.storage);
        map.insert(ChaosModule::Compute, &self.compute);
        map
    }
}

#[derive(Default)]
pub struct ChaosHarness {
    scenarios: Vec<ChaosScenario>,
    state: ChaosReport,
}

impl ChaosHarness {
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
            state: ChaosReport::default(),
        }
    }

    pub fn register(&mut self, scenario: ChaosScenario) {
        self.scenarios.push(scenario);
    }

    pub fn step(&mut self, step: u64, rng: &mut StdRng) -> ChaosReport {
        if self.state.overlay.last_step == 0 {
            self.state.overlay = ChaosModuleState::new(0.97, "bootstrap-overlay".into());
            self.state.storage = ChaosModuleState::new(0.95, "bootstrap-storage".into());
            self.state.compute = ChaosModuleState::new(0.96, "bootstrap-compute".into());
        }
        for module in [
            ChaosModule::Overlay,
            ChaosModule::Storage,
            ChaosModule::Compute,
        ] {
            let state = self.state_for_mut(module);
            state.update_window(step);
            state.last_step = step;
        }

        for scenario in self.scenarios.clone() {
            let module_state = self.state_for_mut(scenario.module);
            module_state.sla_threshold = scenario.sla_threshold;
            module_state.scenario = scenario.name.clone();
            if !scenario.sites.is_empty() {
                module_state
                    .site_readiness
                    .retain(|name, _| scenario.sites.iter().any(|site| site.name == *name));
                for site in &scenario.sites {
                    module_state
                        .site_readiness
                        .entry(site.name.clone())
                        .and_modify(|state| state.set_provider_kind(site.provider_kind))
                        .or_insert_with(|| SiteReadinessState::new(site.provider_kind));
                }
            }
            let mut applied = false;
            for event in &scenario.events {
                if event.is_active(step) {
                    let severity = event.impact() * rng.gen_range(0.8..=1.2);
                    if scenario.sites.is_empty() {
                        module_state.readiness *= (1.0 - severity).max(0.0);
                    } else {
                        for site in &scenario.sites {
                            let penalty = (severity * site.weight * (1.0 + site.latency_penalty))
                                .min(MAX_FAULT_SEVERITY);
                            if let Some(entry) = module_state.site_readiness.get_mut(&site.name) {
                                let updated = (entry.readiness() * (1.0 - penalty)).max(0.0);
                                entry.set_readiness(updated);
                            }
                        }
                    }
                    applied = true;
                }
            }
            if scenario.sites.is_empty() {
                if !applied {
                    module_state.readiness =
                        (module_state.readiness + scenario.recovery_rate).min(1.0);
                }
            } else {
                if !applied {
                    for site in &scenario.sites {
                        if let Some(entry) = module_state.site_readiness.get_mut(&site.name) {
                            let recovery =
                                scenario.recovery_rate * (1.0 - site.latency_penalty).max(0.0);
                            let updated = (entry.readiness() + recovery).min(1.0);
                            entry.set_readiness(updated);
                        }
                    }
                }
                module_state.readiness = module_state
                    .site_readiness
                    .values()
                    .map(|state| state.readiness)
                    .fold(1.0, f64::min);
            }
            if module_state.readiness < scenario.sla_threshold {
                module_state.register_breach();
                warn!(
                    target: "chaos",
                    scenario = %scenario.name,
                    module = %scenario.module,
                    readiness = module_state.readiness,
                    threshold = scenario.sla_threshold,
                    "chaos scenario breached SLA",
                );
            }
        }

        debug!(target: "chaos", step, overlay = %self.state.overlay.readiness, storage = %self.state.storage.readiness, compute = %self.state.compute.readiness, "chaos readiness updated");
        self.state.clone()
    }

    pub fn readiness_snapshot(&self) -> ChaosReport {
        self.state.clone()
    }

    pub fn attestation_drafts(&self, issued_at: u64) -> Vec<ChaosAttestationDraft> {
        let mut drafts = Vec::new();
        for (module, state) in self.state.as_map() {
            let mut site_readiness: Vec<ChaosSiteReadiness> = state
                .site_readiness
                .iter()
                .map(|(site, entry)| ChaosSiteReadiness {
                    site: site.clone(),
                    readiness: entry.readiness(),
                    provider_kind: entry.provider_kind(),
                })
                .collect();
            site_readiness.sort_by(|a, b| a.site.cmp(&b.site));
            let draft = ChaosAttestationDraft {
                scenario: state.scenario.clone(),
                module,
                readiness: state.readiness,
                sla_threshold: state.sla_threshold,
                breaches: state.breaches,
                window_start: state.window_start,
                window_end: state.window_end,
                issued_at,
                site_readiness,
            };
            drafts.push(draft);
        }
        drafts
    }

    fn state_for_mut(&mut self, module: ChaosModule) -> &mut ChaosModuleState {
        match module {
            ChaosModule::Overlay => &mut self.state.overlay,
            ChaosModule::Storage => &mut self.state.storage,
            ChaosModule::Compute => &mut self.state.compute,
        }
    }
}

impl Simulation {
    pub fn chaos_harness(&self) -> &ChaosHarness {
        &self.chaos
    }

    pub fn chaos_harness_mut(&mut self) -> &mut ChaosHarness {
        &mut self.chaos
    }

    pub fn chaos_attestation_drafts(&self, issued_at: u64) -> Vec<ChaosAttestationDraft> {
        self.chaos.attestation_drafts(issued_at)
    }
}

impl ChaosHarness {
    pub fn configure_sites<I>(&mut self, module: ChaosModule, sites: I)
    where
        I: IntoIterator<Item = ChaosSite>,
    {
        let mut normalized: Vec<ChaosSite> = sites.into_iter().collect();
        if normalized.is_empty() {
            if let Some(scenario) = self
                .scenarios
                .iter_mut()
                .find(|scenario| scenario.module == module)
            {
                scenario.sites.clear();
            }
            let state = self.state_for_mut(module);
            state.site_readiness.clear();
            state.readiness = 1.0;
            return;
        }
        let total_weight: f64 = normalized.iter().map(|site| site.weight).sum();
        if total_weight > 0.0 {
            for site in &mut normalized {
                site.weight /= total_weight;
            }
        }
        if let Some(scenario) = self
            .scenarios
            .iter_mut()
            .find(|scenario| scenario.module == module)
        {
            scenario.set_sites(normalized.clone());
        }
        let state = self.state_for_mut(module);
        state.site_readiness = normalized
            .iter()
            .map(|site| {
                (
                    site.name.clone(),
                    SiteReadinessState::new(site.provider_kind),
                )
            })
            .collect();
        state.readiness = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;

    #[test]
    fn distributed_sites_reflect_weighted_penalties() {
        let mut harness = ChaosHarness::new();
        harness.register(
            ChaosScenario::new("compute-grid", ChaosModule::Compute, 0.8, 0.05).add_event(
                ChaosEvent::new(
                    1,
                    2,
                    0.6,
                    ChaosFault::ComputeBackpressure {
                        throttle_ratio: 0.5,
                    },
                ),
            ),
        );
        harness.configure_sites(
            ChaosModule::Compute,
            vec![
                ChaosSite::new("us-east", 0.6, 0.1),
                ChaosSite::new("eu-west", 0.4, 0.2),
            ],
        );
        let mut rng = StdRng::seed_from_u64(4242);
        harness.step(0, &mut rng);
        let report = harness.step(1, &mut rng);
        assert_eq!(report.compute.site_readiness.len(), 2);
        let east = report
            .compute
            .site_readiness
            .get("us-east")
            .map(|state| state.readiness())
            .expect("east site tracked");
        let west = report
            .compute
            .site_readiness
            .get("eu-west")
            .map(|state| state.readiness())
            .expect("west site tracked");
        assert!(report.compute.readiness <= east);
        assert!(report.compute.readiness <= west);
        assert!(report.compute.readiness >= 0.0);
        assert!(report.compute.readiness <= 1.0);
    }
}

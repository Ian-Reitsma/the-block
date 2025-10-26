#![forbid(unsafe_code)]

use crate::Simulation;
use diagnostics::tracing::{debug, warn};
use monitoring_build::ChaosAttestationDraft;
pub use monitoring_build::ChaosModule;
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
        }
    }

    pub fn add_event(mut self, event: ChaosEvent) -> Self {
        self.events.push(event);
        self
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

impl ChaosFault {
    fn severity(&self) -> f64 {
        match self {
            ChaosFault::OverlayPartition { loss_ratio } => loss_ratio.clamp(0.0, 1.0),
            ChaosFault::StorageCorruption { shard_loss } => shard_loss.clamp(0.0, 1.0),
            ChaosFault::ComputeBackpressure { throttle_ratio } => throttle_ratio.clamp(0.0, 1.0),
        }
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
            let mut applied = false;
            for event in &scenario.events {
                if event.is_active(step) {
                    let severity = event.impact() * rng.gen_range(0.8..=1.2);
                    module_state.readiness *= (1.0 - severity).max(0.0);
                    applied = true;
                }
            }
            if !applied {
                module_state.readiness = (module_state.readiness + scenario.recovery_rate).min(1.0);
            } else if module_state.readiness < scenario.sla_threshold {
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
            let draft = ChaosAttestationDraft {
                scenario: state.scenario.clone(),
                module,
                readiness: state.readiness,
                sla_threshold: state.sla_threshold,
                breaches: state.breaches,
                window_start: state.window_start,
                window_end: state.window_end,
                issued_at,
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

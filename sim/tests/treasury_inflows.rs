use foundation_serialization::json;
use foundation_serialization::Deserialize;
#[derive(Default)]
struct TreasuryState {
    balance_ct: u64,
}

impl TreasuryState {
    fn collect(&mut self, reward: u64, percent: u64) {
        self.balance_ct += reward * percent / 100;
    }
}

#[derive(Deserialize)]
struct Scenario {
    steps: u64,
    treasury_percent: u64,
    proposals: Vec<Proposal>,
}

#[derive(Deserialize)]
struct Proposal {
    id: u64,
    deps: Vec<u64>,
    subsidy: u64,
}

#[test]
fn treasury_inflows_scenario_models_outcomes() {
    let text = std::fs::read_to_string("governance/treasury_inflows.json").unwrap();
    let s: Scenario = json::from_str(&text).unwrap();
    let mut activated = std::collections::HashSet::new();
    let mut treasury = TreasuryState::default();
    for p in &s.proposals {
        if p.deps.iter().all(|d| activated.contains(d)) {
            treasury.collect(p.subsidy, s.treasury_percent);
            activated.insert(p.id);
        }
    }
    assert_eq!(treasury.balance_ct, 25);
}

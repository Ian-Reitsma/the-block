use foundation_serialization::json::{self, Value};
#[derive(Default)]
struct TreasuryState {
    balance: u64,
}

impl TreasuryState {
    fn collect(&mut self, reward: u64, percent: u64) {
        self.balance += reward * percent / 100;
    }
}

#[test]
fn treasury_inflows_scenario_models_outcomes() {
    let text = std::fs::read_to_string("governance/treasury_inflows.json").unwrap();
    let scenario: Value = json::value_from_str(&text).unwrap();
    let treasury_percent = scenario
        .as_object()
        .and_then(|map| map.get("treasury_percent"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let proposals = scenario
        .as_object()
        .and_then(|map| map.get("proposals"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut activated = std::collections::HashSet::new();
    let mut treasury = TreasuryState::default();
    for proposal in &proposals {
        let Some(map) = proposal.as_object() else {
            continue;
        };
        let id = map.get("id").and_then(Value::as_u64).unwrap_or_default();
        let deps = map
            .get("deps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let subsidy = map
            .get("subsidy")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if deps
            .iter()
            .filter_map(Value::as_u64)
            .all(|dep| activated.contains(&dep))
        {
            treasury.collect(subsidy, treasury_percent);
            activated.insert(id);
        }
    }
    assert_eq!(treasury.balance, 25);
}

use ledger::{TokenRegistry, Emission};

#[test]
fn registry_orders_tokens() {
    let mut reg = TokenRegistry::new();
    reg.register("ZZZ", Emission::Fixed(1));
    reg.register("AAA", Emission::Fixed(1));
    let list = reg.list();
    assert_eq!(list, vec!["AAA".to_string(), "ZZZ".to_string()]);
}

#[test]
fn linear_emission_accumulates() {
    let mut reg = TokenRegistry::new();
    reg.register("TKN", Emission::Linear { initial: 10, rate: 2 });
    let t = reg.get("TKN").unwrap();
    assert_eq!(t.emission.supply_at(5), 20);
}

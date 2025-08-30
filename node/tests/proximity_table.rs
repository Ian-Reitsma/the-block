use the_block::localnet::{validate_proximity, DeviceClass};

#[test]
fn proximity_table_enforces_corridors() {
    assert!(validate_proximity(DeviceClass::Phone, -70, 100));
    assert!(!validate_proximity(DeviceClass::Phone, -90, 100));
    assert!(validate_proximity(DeviceClass::Laptop, -80, 150));
}

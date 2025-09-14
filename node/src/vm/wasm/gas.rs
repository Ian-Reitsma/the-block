/// Conversion between abstract gas units and Wasmtime fuel.
pub const FUEL_PER_GAS: u64 = 100;

#[must_use]
pub fn to_fuel(gas: u64) -> u64 {
    gas.saturating_mul(FUEL_PER_GAS)
}

#[must_use]
pub fn from_fuel(fuel: u64) -> u64 {
    fuel / FUEL_PER_GAS
}

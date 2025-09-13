use ledger::address;

/// Return the shard identifier for a given address.
pub fn shard_of(addr: &str) -> u16 {
    address::shard_id(addr)
}

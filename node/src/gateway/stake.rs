use crate::gateway::dns;
use crate::web::gateway::StakeTable;
use std::sync::Arc;

/// Stake table implementation that delegates to `dns::domain_has_stake`.
#[derive(Clone, Default)]
pub struct DnsStakeTable;

impl DnsStakeTable {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl StakeTable for DnsStakeTable {
    fn has_stake(&self, domain: &str) -> bool {
        dns::domain_has_stake(domain)
    }
}

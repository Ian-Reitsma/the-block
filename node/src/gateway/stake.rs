use crate::gateway::dns;
use crate::web::gateway::StakeTable;
use std::{collections::HashSet, env, sync::Arc};

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

pub fn with_env_overrides(
    base: Arc<dyn StakeTable + Send + Sync>,
) -> Arc<dyn StakeTable + Send + Sync> {
    let allowlist = load_override_domains();
    if allowlist.is_empty() {
        base
    } else {
        Arc::new(OverrideStakeTable { base, allowlist })
    }
}

fn load_override_domains() -> HashSet<String> {
    env::var("TB_GATEWAY_STAKE_ALLOWLIST")
        .ok()
        .unwrap_or_default()
        .split(',')
        .filter_map(|entry| {
            let candidate = entry.trim();
            if candidate.is_empty() {
                None
            } else {
                Some(candidate.to_string())
            }
        })
        .collect()
}

struct OverrideStakeTable {
    base: Arc<dyn StakeTable + Send + Sync>,
    allowlist: HashSet<String>,
}

impl StakeTable for OverrideStakeTable {
    fn has_stake(&self, domain: &str) -> bool {
        self.base.has_stake(domain) || self.allowlist.contains(domain)
    }
}

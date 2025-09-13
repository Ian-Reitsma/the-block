use serde::{Deserialize, Serialize};

pub type ShardId = u16;

/// Encode an account address with shard prefix as `hhhh:acct` where `hhhh` is hex shard id.
pub fn encode(shard: ShardId, account: &str) -> String {
    format!("{:04x}:{}", shard, account)
}

/// Extract the shard identifier from an encoded address.
pub fn shard_id(addr: &str) -> ShardId {
    addr.split(':')
        .next()
        .and_then(|s| u16::from_str_radix(s, 16).ok())
        .unwrap_or(0)
}

/// Extract the account portion from an encoded address.
pub fn account(addr: &str) -> &str {
    addr.split(':').nth(1).unwrap_or(addr)
}

/// Parsed address type with shard and account fields.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Address {
    pub shard: ShardId,
    pub account: String,
}

impl Address {
    /// Parse a string address into a structured form.
    pub fn parse(addr: &str) -> Self {
        Self {
            shard: shard_id(addr),
            account: account(addr).to_string(),
        }
    }
}

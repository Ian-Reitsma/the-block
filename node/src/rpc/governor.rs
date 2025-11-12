use super::{rpc_error, status_value, RpcError};
use crate::launch_governor;
use foundation_serialization::json::{self, Value};
use std::sync::Arc;

pub fn status(handle: Option<Arc<launch_governor::GovernorHandle>>) -> Result<Value, RpcError> {
    match handle {
        Some(governor) => json::to_value(governor.status())
            .map_err(|_| rpc_error(-32603, "failed to encode governor status")),
        None => Ok(status_value("disabled")),
    }
}

pub fn decisions(
    handle: Option<Arc<launch_governor::GovernorHandle>>,
    limit: usize,
) -> Result<Value, RpcError> {
    match handle {
        Some(governor) => json::to_value(governor.decisions(limit))
            .map_err(|_| rpc_error(-32603, "failed to encode governor decisions")),
        None => Ok(Value::Array(Vec::new())),
    }
}

pub fn snapshot(
    handle: Option<Arc<launch_governor::GovernorHandle>>,
    epoch: u64,
) -> Result<Value, RpcError> {
    match handle {
        Some(governor) => Ok(governor
            .snapshot(epoch)
            .unwrap_or_else(|| status_value("not_found"))),
        None => Ok(status_value("disabled")),
    }
}

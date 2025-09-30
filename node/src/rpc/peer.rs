use crate::net::uptime;
use hex;
use jsonrpc_core::{BoxFuture, Params, Result};

pub fn rebate_status(params: Params) -> Result<BoxFuture<Result<String>>> {
    let (peer, threshold, epoch): (String, u64, u64) = params.parse()?;
    let peer_id = uptime::peer_from_base58(&peer)
        .map_err(|e| jsonrpc_core::Error::invalid_params(e.to_string()))?;
    let eligible = uptime::eligible(&peer_id, threshold, epoch);
    Ok(Box::pin(async move { Ok(eligible.to_string()) }))
}

pub fn rebate_claim(params: Params) -> Result<BoxFuture<Result<String>>> {
    let (peer, threshold, epoch, reward): (String, u64, u64, u64) = params.parse()?;
    let peer_id = uptime::peer_from_base58(&peer)
        .map_err(|e| jsonrpc_core::Error::invalid_params(e.to_string()))?;
    let res = uptime::claim(peer_id, threshold, epoch, reward)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0".into());
    Ok(Box::pin(async move { Ok(res) }))
}

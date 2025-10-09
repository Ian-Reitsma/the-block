use crate::net::uptime;
use foundation_rpc::RpcError;

fn parse_peer(peer: &str) -> Result<uptime::PeerId, RpcError> {
    uptime::peer_from_base58(peer).map_err(|_| RpcError::new(-32602, "invalid peer"))
}

pub fn rebate_status(peer: &str, threshold: u64, epoch: u64) -> Result<String, RpcError> {
    let peer_id = parse_peer(peer)?;
    let eligible = uptime::eligible(&peer_id, threshold, epoch);
    Ok(eligible.to_string())
}

pub fn rebate_claim(
    peer: &str,
    threshold: u64,
    epoch: u64,
    reward: u64,
) -> Result<String, RpcError> {
    let peer_id = parse_peer(peer)?;
    let res = uptime::claim(peer_id, threshold, epoch, reward)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0".into());
    Ok(res)
}

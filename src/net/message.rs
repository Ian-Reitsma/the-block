use crate::{Block, SignedTransaction};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Network messages exchanged between peers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    /// Advertise known peers.
    Hello(Vec<SocketAddr>),
    /// Broadcast a transaction to be relayed and mined.
    Tx(SignedTransaction),
    /// Broadcast a newly mined block.
    Block(Block),
    /// Share an entire chain snapshot for fork resolution.
    Chain(Vec<Block>),
}

use ledger::address::ShardId;
use std::collections::{HashSet, VecDeque};

/// Simple in-memory inter-shard message queue with replay protection.
#[derive(Default)]
pub struct MessageQueue {
    seen: HashSet<Vec<u8>>,
    queue: VecDeque<(ShardId, Vec<u8>)>,
}

impl MessageQueue {
    /// Enqueue a message for a destination shard.
    pub fn enqueue(&mut self, dest: ShardId, msg: Vec<u8>) {
        if self.seen.insert(msg.clone()) {
            self.queue.push_back((dest, msg));
        }
    }

    /// Pop the next pending message.
    pub fn dequeue(&mut self) -> Option<(ShardId, Vec<u8>)> {
        self.queue.pop_front()
    }
}

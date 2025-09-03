use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HopProof {
    pub relay: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    pub payload: Vec<u8>,
    pub proofs: Vec<HopProof>,
}

pub struct RangeBoost {
    queue: VecDeque<Bundle>,
}

impl RangeBoost {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, payload: Vec<u8>) {
        self.queue.push_back(Bundle {
            payload,
            proofs: vec![],
        });
    }

    pub fn record_proof(&mut self, idx: usize, proof: HopProof) {
        if let Some(bundle) = self.queue.get_mut(idx) {
            bundle.proofs.push(proof);
        }
    }

    pub fn dequeue(&mut self) -> Option<Bundle> {
        self.queue.pop_front()
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_roundtrip() {
        let mut rb = RangeBoost::new();
        rb.enqueue(vec![1, 2, 3]);
        assert_eq!(rb.pending(), 1);
        rb.record_proof(0, HopProof { relay: "r1".into() });
        let b = rb.dequeue().unwrap();
        assert_eq!(b.payload, vec![1, 2, 3]);
        assert_eq!(b.proofs.len(), 1);
    }
}

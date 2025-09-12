#[derive(Default)]
pub struct ReorgTracker {
    pub hashes: Vec<String>,
}

impl ReorgTracker {
    pub fn record(&mut self, hash: &str) {
        self.hashes.push(hash.to_string());
    }

    pub fn rollback(&mut self, depth: usize) {
        if depth <= self.hashes.len() {
            self.hashes.truncate(self.hashes.len() - depth);
        } else {
            self.hashes.clear();
        }
    }
}

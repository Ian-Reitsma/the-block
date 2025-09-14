pub fn summarize_block(height: u64, txs: usize) -> String {
    format!("Block {height} contains {txs} transactions")
}

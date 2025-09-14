use crate::{blockchain::process::{validate_and_apply, ExecutionContext}, Block, Blockchain};
#[cfg(feature = "telemetry")]
use crate::telemetry::PARTITION_RECOVER_BLOCKS;

/// Replay `blocks` against `chain` after a partition heals.
pub fn replay_blocks(chain: &mut Blockchain, blocks: &[Block]) {
    for b in blocks {
        if let Ok(deltas) = validate_and_apply(chain, b) {
            let mut ctx = ExecutionContext::new(chain);
            if ctx.apply(deltas).is_ok() {
                let _ = ctx.commit();
            }
        }
    }
    #[cfg(feature = "telemetry")]
    PARTITION_RECOVER_BLOCKS.inc_by(blocks.len() as u64);
}

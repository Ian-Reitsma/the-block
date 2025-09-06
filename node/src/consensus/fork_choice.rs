use std::cmp::Ordering;

/// Metadata describing a chain tip used for fork choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipMeta {
    /// Block height of the tip.
    pub height: u64,
    /// Cumulative weight associated with the tip.
    pub weight: u128,
    /// Hash of the tip block.
    pub tip_hash: [u8; 32],
    /// Height of the latest finalized PoS checkpoint referenced by this chain.
    pub checkpoint_height: u64,
}

/// Deterministically choose the preferred tip between `a` and `b`.
///
/// Ordering precedence is:
/// 1. Higher block height wins.
/// 2. If heights match, higher cumulative weight wins.
/// 3. If both match, lexicographically greater tip hash wins, ensuring
///    a total order and deterministic tie-break.
pub fn choose_tip(a: &TipMeta, b: &TipMeta) -> Ordering {
    #[cfg(feature = "telemetry")]
    let span = if crate::telemetry::should_log("consensus") {
        Some(crate::log_context!(block = a.height.max(b.height)))
    } else {
        None
    };
    #[cfg(feature = "telemetry")]
    if let Some(ref s) = span {
        tracing::info!(parent: s, a_height = a.height, b_height = b.height, "fork_choice_start");
    }
    use Ordering::*;
    let res = match a.checkpoint_height.cmp(&b.checkpoint_height) {
        Greater => Greater,
        Less => Less,
        Equal => match a.height.cmp(&b.height) {
            Greater => Greater,
            Less => Less,
            Equal => match a.weight.cmp(&b.weight) {
                Greater => Greater,
                Less => Less,
                Equal => a.tip_hash.cmp(&b.tip_hash),
            },
        },
    };
    #[cfg(feature = "telemetry")]
    if let Some(s) = span {
        tracing::info!(parent: &s, ?res, "fork_choice_end");
    }
    res
}

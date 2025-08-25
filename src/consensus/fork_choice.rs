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
}

/// Deterministically choose the preferred tip between `a` and `b`.
///
/// Ordering precedence is:
/// 1. Higher block height wins.
/// 2. If heights match, higher cumulative weight wins.
/// 3. If both match, lexicographically greater tip hash wins, ensuring
///    a total order and deterministic tie-break.
pub fn choose_tip(a: &TipMeta, b: &TipMeta) -> Ordering {
    use Ordering::*;
    match a.height.cmp(&b.height) {
        Greater => Greater,
        Less => Less,
        Equal => match a.weight.cmp(&b.weight) {
            Greater => Greater,
            Less => Less,
            Equal => a.tip_hash.cmp(&b.tip_hash),
        },
    }
}

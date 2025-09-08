use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, PartialEq, PartialOrd)]
struct F64(pub f64);

impl Eq for F64 {}

impl Ord for F64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

/// Approximate top-k selection using a Count-Sketch style heap.
/// Values are streamed once and the heap maintains the k largest magnitudes.
pub fn topk(values: &[f64], k: usize) -> Vec<usize> {
    let mut heap: BinaryHeap<(Reverse<F64>, usize)> = BinaryHeap::new();
    for (idx, &v) in values.iter().enumerate() {
        let mag = F64(v.abs());
        if heap.len() < k {
            heap.push((Reverse(mag), idx));
        } else if let Some((Reverse(m), _)) = heap.peek() {
            if mag > *m {
                heap.pop();
                heap.push((Reverse(mag), idx));
            }
        }
    }
    let mut res: Vec<_> = heap.into_iter().map(|(_, i)| i).collect();
    res.sort();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_topk() {
        let v = vec![1.0, -5.0, 3.0, 10.0, -2.0];
        let top = topk(&v, 2);
        assert_eq!(top, vec![1, 3]);
    }
}

use blake3;
use crate::consensus::committee::topk::topk as sketch_topk;

pub fn unruh_extract(vrf: &[u8], prev_block: &[u8]) -> [u8;32] {
    let mut h = blake3::Hasher::new();
    h.update(vrf);
    h.update(prev_block);
    let out = h.finalize();
    *out.as_bytes()
}

pub fn sample_committee(vrf: &[u8], prev_block: &[u8], n: usize, k: usize) -> Vec<usize> {
    assert!(n.is_power_of_two());
    let seed = unruh_extract(vrf, prev_block);
    let mut vec = vec![0f64; n];
    for i in 0..n {
        let bit = (seed[i / 8] >> (i % 8)) & 1;
        vec[i] = if bit == 1 { 1.0 } else { -1.0 };
    }
    hadamard(&mut vec);
    sketch_topk(&vec, k)
}

fn hadamard(a: &mut [f64]) {
    let mut len = 1;
    let n = a.len();
    while len < n {
        for i in (0..n).step_by(len * 2) {
            for j in 0..len {
                let x = a[i + j];
                let y = a[i + j + len];
                a[i + j] = x + y;
                a[i + j + len] = x - y;
            }
        }
        len *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committee_size() {
        let vrf = [1u8;32];
        let prev = [2u8;32];
        let sel = sample_committee(&vrf, &prev, 8, 3);
        assert_eq!(sel.len(),3);
        // Ensure selected indices correspond to largest magnitudes as a sanity check
        let mut vec = vec![0f64;8];
        let seed = unruh_extract(&vrf,&prev);
        for i in 0..8 { let bit = (seed[i/8]>>(i%8))&1; vec[i]=if bit==1{1.0}else{-1.0}; }
        hadamard(&mut vec);
        let mut idx: Vec<usize>=(0..8).collect();
        idx.sort_by(|a,b| vec[*b].abs().partial_cmp(&vec[*a].abs()).unwrap());
        idx.truncate(3);
        let mut sel_sorted=sel.clone(); sel_sorted.sort();
        let mut idx_sorted=idx.clone(); idx_sorted.sort();
        assert_eq!(sel_sorted, idx_sorted);
    }
}

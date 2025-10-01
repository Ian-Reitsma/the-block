use super::{
    ErasureBatch, ErasureCoder, ErasureError, ErasureMetadata, ErasureShard, ErasureShardKind,
};

const PRIMITIVE_POLY: u8 = 0x1d;
const FIELD_SIZE: usize = 256;
const FIELD_ORDER: usize = FIELD_SIZE - 1;

#[derive(Clone)]
struct GfTables {
    exp: [u8; FIELD_SIZE * 2],
    log: [u8; FIELD_SIZE],
}

impl GfTables {
    const fn build() -> Self {
        let mut exp = [0u8; FIELD_SIZE * 2];
        let mut log = [0u8; FIELD_SIZE];
        let mut value: u8 = 1;
        let mut i = 0usize;
        while i < FIELD_ORDER {
            exp[i] = value;
            log[value as usize] = i as u8;
            value = mul_no_tables(value, 2);
            i += 1;
        }
        let mut j = FIELD_ORDER;
        while j < exp.len() {
            exp[j] = exp[j - FIELD_ORDER];
            j += 1;
        }
        Self { exp, log }
    }
}

const TABLES: GfTables = GfTables::build();

const fn mul_no_tables(mut a: u8, mut b: u8) -> u8 {
    let mut product: u8 = 0;
    let mut i = 0;
    while i < 8 {
        if (b & 1) != 0 {
            product ^= a;
        }
        let carry = a & 0x80;
        a <<= 1;
        if carry != 0 {
            a ^= PRIMITIVE_POLY;
        }
        b >>= 1;
        i += 1;
    }
    product
}

#[inline]
fn gf_add(a: u8, b: u8) -> u8 {
    a ^ b
}

#[inline]
fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let log_a = TABLES.log[a as usize] as usize;
    let log_b = TABLES.log[b as usize] as usize;
    TABLES.exp[log_a + log_b]
}

#[inline]
fn gf_inv(a: u8) -> Option<u8> {
    if a == 0 {
        None
    } else {
        let idx = FIELD_ORDER - TABLES.log[a as usize] as usize;
        Some(TABLES.exp[idx])
    }
}

#[inline]
fn gf_scale_row(row: &mut [u8], factor: u8) {
    if factor == 0 {
        row.fill(0);
        return;
    }
    for value in row.iter_mut() {
        *value = gf_mul(*value, factor);
    }
}

#[inline]
fn gf_axpy(target: &mut [u8], factor: u8, source: &[u8]) {
    if factor == 0 {
        return;
    }
    for (dst, src) in target.iter_mut().zip(source.iter()) {
        *dst = gf_add(*dst, gf_mul(factor, *src));
    }
}

#[derive(Clone)]
pub struct InhouseReedSolomon {
    data: usize,
    parity: usize,
    generator: Vec<Vec<u8>>,
}

impl InhouseReedSolomon {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self, ErasureError> {
        if data_shards == 0 {
            return Err(ErasureError::InvalidShardCount {
                expected: 1,
                actual: 0,
            });
        }
        let total = data_shards + parity_shards;
        if total >= FIELD_SIZE {
            return Err(ErasureError::InvalidShardCount {
                expected: FIELD_SIZE - 1,
                actual: total,
            });
        }
        let mut generator = vec![vec![0u8; data_shards]; total];
        for i in 0..data_shards {
            generator[i][i] = 1;
        }
        for row in 0..parity_shards {
            let base = TABLES.exp[row];
            let mut coeff = 1u8;
            for col in 0..data_shards {
                generator[data_shards + row][col] = coeff;
                coeff = gf_mul(coeff, base);
            }
        }
        Ok(Self {
            data: data_shards,
            parity: parity_shards,
            generator,
        })
    }

    fn total(&self) -> usize {
        self.data + self.parity
    }

    fn generator_row(&self, index: usize) -> &[u8] {
        &self.generator[index]
    }

    fn solve_system(
        &self,
        mut matrix: Vec<Vec<u8>>,
        mut values: Vec<Vec<u8>>,
        shard_len: usize,
    ) -> Result<Vec<Vec<u8>>, ErasureError> {
        let mut rank = 0usize;
        for col in 0..self.data {
            if rank >= matrix.len() {
                break;
            }
            let mut pivot = None;
            for row in rank..matrix.len() {
                if matrix[row][col] != 0 {
                    pivot = Some(row);
                    break;
                }
            }
            let Some(pivot_row) = pivot else {
                continue;
            };
            if pivot_row != rank {
                matrix.swap(rank, pivot_row);
                values.swap(rank, pivot_row);
            }
            let pivot_val = matrix[rank][col];
            let Some(inv) = gf_inv(pivot_val) else {
                return Err(ErasureError::ReconstructionFailed(
                    "singular pivot".to_string(),
                ));
            };
            gf_scale_row(&mut matrix[rank], inv);
            gf_scale_row(&mut values[rank], inv);
            let pivot_row_matrix = matrix[rank].clone();
            let pivot_row_values = values[rank].clone();
            for row in 0..matrix.len() {
                if row == rank {
                    continue;
                }
                let factor = matrix[row][col];
                if factor == 0 {
                    continue;
                }
                gf_axpy(&mut matrix[row], factor, &pivot_row_matrix);
                matrix[row][col] = 0;
                gf_axpy(&mut values[row], factor, &pivot_row_values);
            }
            rank += 1;
            if rank == self.data {
                break;
            }
        }
        if rank < self.data {
            return Err(ErasureError::ReconstructionFailed(
                "insufficient independent shards".to_string(),
            ));
        }
        let mut solution = Vec::with_capacity(self.data);
        for row in 0..self.data {
            let mut shard = values[row].clone();
            shard.truncate(shard_len);
            if shard.len() < shard_len {
                shard.resize(shard_len, 0);
            }
            solution.push(shard);
        }
        Ok(solution)
    }
}

impl ErasureCoder for InhouseReedSolomon {
    fn algorithm(&self) -> &'static str {
        "reed-solomon"
    }

    fn encode(&self, data: &[u8]) -> Result<ErasureBatch, ErasureError> {
        let shard_len = if data.is_empty() {
            0
        } else {
            (data.len() + self.data - 1) / self.data
        };
        let total = self.total();
        let mut shards: Vec<Vec<u8>> = Vec::with_capacity(total);
        for i in 0..self.data {
            let start = i * shard_len;
            let end = usize::min(start + shard_len, data.len());
            let mut shard = vec![0u8; shard_len];
            if start < end {
                shard[..end - start].copy_from_slice(&data[start..end]);
            }
            shards.push(shard);
        }
        for row in 0..self.parity {
            let mut parity = vec![0u8; shard_len];
            let coeffs = self.generator_row(self.data + row);
            for (col, coeff) in coeffs.iter().enumerate() {
                if *coeff == 0 {
                    continue;
                }
                let data_shard = &shards[col];
                for (dst, src) in parity.iter_mut().zip(data_shard.iter()) {
                    *dst = gf_add(*dst, gf_mul(*coeff, *src));
                }
            }
            shards.push(parity);
        }
        let metadata = ErasureMetadata {
            data_shards: self.data,
            parity_shards: self.parity,
            shard_len,
            original_len: data.len(),
        };
        let mut out = Vec::with_capacity(total);
        for (index, shard) in shards.into_iter().enumerate() {
            let kind = if index < self.data {
                ErasureShardKind::Data
            } else {
                ErasureShardKind::Parity
            };
            out.push(ErasureShard {
                index,
                kind,
                bytes: shard,
            });
        }
        Ok(ErasureBatch {
            metadata,
            shards: out,
        })
    }

    fn reconstruct(
        &self,
        metadata: &ErasureMetadata,
        shards: &[Option<ErasureShard>],
    ) -> Result<Vec<u8>, ErasureError> {
        let total = metadata.data_shards + metadata.parity_shards;
        if shards.len() != total {
            return Err(ErasureError::InvalidShardCount {
                expected: total,
                actual: shards.len(),
            });
        }
        let mut available: Vec<(usize, Vec<u8>)> = Vec::new();
        for (idx, maybe_shard) in shards.iter().enumerate() {
            if let Some(shard) = maybe_shard {
                if shard.index != idx {
                    return Err(ErasureError::InvalidShardIndex {
                        index: shard.index,
                        total,
                    });
                }
                let mut bytes = shard.bytes.clone();
                if bytes.len() < metadata.shard_len {
                    bytes.resize(metadata.shard_len, 0);
                }
                available.push((idx, bytes));
            }
        }
        if available.len() < metadata.data_shards {
            return Err(ErasureError::InsufficientShards {
                expected: metadata.data_shards,
                available: available.len(),
            });
        }
        let shard_len = metadata.shard_len;
        let mut matrix = Vec::with_capacity(available.len());
        let mut values = Vec::with_capacity(available.len());
        for (index, bytes) in available.into_iter() {
            matrix.push(self.generator_row(index).to_vec());
            values.push(bytes);
        }
        let solved = self.solve_system(matrix, values, shard_len)?;
        let mut recovered = Vec::with_capacity(metadata.original_len);
        for shard in solved.into_iter().take(metadata.data_shards) {
            recovered.extend_from_slice(&shard);
        }
        recovered.truncate(metadata.original_len);
        Ok(recovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_is_systematic() {
        let coder = InhouseReedSolomon::new(4, 2).unwrap();
        for row in 0..coder.data {
            let mut expected = vec![0u8; coder.data];
            expected[row] = 1;
            assert_eq!(coder.generator_row(row), expected.as_slice());
        }
    }

    #[test]
    fn encode_and_reconstruct_roundtrip() {
        let coder = InhouseReedSolomon::new(4, 2).unwrap();
        let data = (0u8..64).collect::<Vec<_>>();
        let batch = coder.encode(&data).unwrap();
        let mut shards: Vec<Option<ErasureShard>> = batch.shards.into_iter().map(Some).collect();
        shards[1] = None;
        shards[4] = None;
        let recovered = coder
            .reconstruct(&batch.metadata, &shards)
            .expect("should recover");
        assert_eq!(recovered, data);
    }
}

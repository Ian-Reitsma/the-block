use core::ops::{Index, IndexMut};

/// Dense column vector backed by a fixed-size array.
#[derive(Clone, Debug, PartialEq)]
pub struct Vector<const N: usize> {
    data: [f64; N],
}

impl<const N: usize> Vector<N> {
    /// Create a zero-initialised vector.
    pub fn zeros() -> Self {
        Self { data: [0.0; N] }
    }

    /// Construct a vector from an array.
    pub fn from_array(data: [f64; N]) -> Self {
        Self { data }
    }

    /// Construct a vector from a slice.
    pub fn from_slice(slice: &[f64]) -> Self {
        assert_eq!(slice.len(), N, "slice length mismatch");
        let mut data = [0.0; N];
        data.copy_from_slice(slice);
        Self { data }
    }

    /// Return the underlying storage as a slice.
    pub fn as_slice(&self) -> &[f64] {
        &self.data
    }

    /// Return the underlying storage as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [f64] {
        &mut self.data
    }

    /// Convert the vector into an owned array.
    pub fn into_array(self) -> [f64; N] {
        self.data
    }

    /// Convert the vector into an owned vector.
    pub fn to_vec(&self) -> Vec<f64> {
        self.data.to_vec()
    }

    /// Element-wise addition returning a new vector.
    pub fn add(&self, other: &Self) -> Self {
        let mut out = [0.0; N];
        for i in 0..N {
            out[i] = self.data[i] + other.data[i];
        }
        Self { data: out }
    }

    /// Element-wise subtraction returning a new vector.
    pub fn sub(&self, other: &Self) -> Self {
        let mut out = [0.0; N];
        for i in 0..N {
            out[i] = self.data[i] - other.data[i];
        }
        Self { data: out }
    }

    /// In-place element-wise addition.
    pub fn add_assign(&mut self, other: &Self) {
        for i in 0..N {
            self.data[i] += other.data[i];
        }
    }

    /// Return a scaled copy of the vector.
    pub fn scale(&self, factor: f64) -> Self {
        let mut out = [0.0; N];
        for i in 0..N {
            out[i] = self.data[i] * factor;
        }
        Self { data: out }
    }
}

impl<const N: usize> Default for Vector<N> {
    fn default() -> Self {
        Self::zeros()
    }
}

impl<const N: usize> Index<usize> for Vector<N> {
    type Output = f64;

    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}

impl<const N: usize> IndexMut<usize> for Vector<N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}

/// Dense row-major matrix backed by a fixed-size array.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix<const R: usize, const C: usize> {
    data: [[f64; C]; R],
}

impl<const R: usize, const C: usize> Matrix<R, C> {
    /// Construct a zero matrix.
    pub fn zeros() -> Self {
        Self {
            data: [[0.0; C]; R],
        }
    }

    /// Construct a matrix from a closure producing each element.
    pub fn from_fn<F: FnMut(usize, usize) -> f64>(mut f: F) -> Self {
        let mut data = [[0.0; C]; R];
        for r in 0..R {
            for c in 0..C {
                data[r][c] = f(r, c);
            }
        }
        Self { data }
    }

    /// Construct a matrix from a row-major slice.
    pub fn from_row_major(slice: &[f64]) -> Self {
        assert_eq!(slice.len(), R * C, "slice length mismatch");
        let mut data = [[0.0; C]; R];
        for r in 0..R {
            for c in 0..C {
                data[r][c] = slice[r * C + c];
            }
        }
        Self { data }
    }

    /// Return the matrix as a flattened slice (row-major).
    pub fn as_slice(&self) -> &[f64] {
        unsafe { core::slice::from_raw_parts(self.data.as_ptr() as *const f64, R * C) }
    }

    /// Return the matrix as a mutable flattened slice (row-major).
    pub fn as_mut_slice(&mut self) -> &mut [f64] {
        unsafe { core::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut f64, R * C) }
    }

    /// Convert into an owned vector in row-major order.
    pub fn to_vec(&self) -> Vec<f64> {
        self.as_slice().to_vec()
    }

    /// Matrix transpose.
    pub fn transpose(&self) -> Matrix<C, R> {
        Matrix::<C, R>::from_fn(|r, c| self.data[c][r])
    }

    /// Add two matrices of the same dimensions.
    pub fn add(&self, other: &Self) -> Self {
        let mut data = [[0.0; C]; R];
        for r in 0..R {
            for c in 0..C {
                data[r][c] = self.data[r][c] + other.data[r][c];
            }
        }
        Self { data }
    }

    /// Subtract another matrix from this matrix.
    pub fn sub(&self, other: &Self) -> Self {
        let mut data = [[0.0; C]; R];
        for r in 0..R {
            for c in 0..C {
                data[r][c] = self.data[r][c] - other.data[r][c];
            }
        }
        Self { data }
    }

    /// Scale the matrix by a constant factor.
    pub fn scale(&self, factor: f64) -> Self {
        let mut data = [[0.0; C]; R];
        for r in 0..R {
            for c in 0..C {
                data[r][c] = self.data[r][c] * factor;
            }
        }
        Self { data }
    }

    /// Matrix multiplication.
    pub fn mul_matrix<const K: usize>(&self, rhs: &Matrix<C, K>) -> Matrix<R, K> {
        Matrix::<R, K>::from_fn(|r, k| {
            let mut sum = 0.0;
            for c in 0..C {
                sum += self.data[r][c] * rhs.data[c][k];
            }
            sum
        })
    }

    /// Matrix-vector multiplication.
    pub fn mul_vector(&self, rhs: &Vector<C>) -> Vector<R> {
        let mut data = [0.0; R];
        for r in 0..R {
            let mut sum = 0.0;
            for c in 0..C {
                sum += self.data[r][c] * rhs[c];
            }
            data[r] = sum;
        }
        Vector { data }
    }
}

impl<const N: usize> Matrix<N, N> {
    /// Identity matrix.
    pub fn identity() -> Self {
        Self::from_fn(|r, c| if r == c { 1.0 } else { 0.0 })
    }

    /// Trace of the matrix.
    pub fn trace(&self) -> f64 {
        let mut sum = 0.0;
        for i in 0..N {
            sum += self.data[i][i];
        }
        sum
    }

    /// Attempt to invert the matrix using Gauss-Jordan elimination.
    pub fn try_inverse(&self) -> Option<Self> {
        const EPS: f64 = 1e-12;
        let mut a = self.data;
        let mut inv = Self::identity().data;

        for i in 0..N {
            // Pivot selection.
            let mut pivot_row = i;
            let mut pivot_val = a[i][i].abs();
            for r in (i + 1)..N {
                let candidate = a[r][i].abs();
                if candidate > pivot_val {
                    pivot_val = candidate;
                    pivot_row = r;
                }
            }
            if pivot_val < EPS {
                return None;
            }
            if pivot_row != i {
                a.swap(pivot_row, i);
                inv.swap(pivot_row, i);
            }

            let pivot = a[i][i];
            for c in 0..N {
                a[i][c] /= pivot;
                inv[i][c] /= pivot;
            }

            for r in 0..N {
                if r == i {
                    continue;
                }
                let factor = a[r][i];
                for c in 0..N {
                    a[r][c] -= factor * a[i][c];
                    inv[r][c] -= factor * inv[i][c];
                }
            }
        }

        Some(Self { data: inv })
    }
}

impl<const R: usize, const C: usize> Default for Matrix<R, C> {
    fn default() -> Self {
        Self::zeros()
    }
}

impl<const R: usize, const C: usize> Index<(usize, usize)> for Matrix<R, C> {
    type Output = f64;

    fn index(&self, index: (usize, usize)) -> &Self::Output {
        &self.data[index.0][index.1]
    }
}

impl<const R: usize, const C: usize> IndexMut<(usize, usize)> for Matrix<R, C> {
    fn index_mut(&mut self, index: (usize, usize)) -> &mut Self::Output {
        &mut self.data[index.0][index.1]
    }
}

#[cfg(test)]
mod tests {
    use super::{Matrix, Vector};
    use crate::testing::{assert_close, assert_close_with};

    #[test]
    fn vector_addition() {
        let a = Vector::<4>::from_array([1.0, 2.0, 3.0, 4.0]);
        let b = Vector::<4>::from_array([0.5, -1.0, 0.0, 2.0]);
        let c = a.add(&b);
        assert_close(c[0], 1.5);
        assert_close(c[1], 1.0);
        assert_close(c[2], 3.0);
        assert_close(c[3], 6.0);
    }

    #[test]
    fn matrix_mul_vector() {
        let m = Matrix::<2, 3>::from_row_major(&[1.0, 2.0, 3.0, 0.0, -1.0, 4.0]);
        let v = Vector::<3>::from_array([1.0, 0.0, 2.0]);
        let result = m.mul_vector(&v);
        assert_close(result[0], 7.0);
        assert_close(result[1], 8.0);
    }

    #[test]
    fn matrix_inverse_identity() {
        let mut m = Matrix::<3, 3>::identity();
        m[(0, 1)] = 2.0;
        m[(1, 2)] = -1.0;
        m[(2, 0)] = -0.5;
        let inv = m.try_inverse().unwrap();
        let prod = m.mul_matrix(&inv);
        for r in 0..3 {
            for c in 0..3 {
                let expected = if r == c { 1.0 } else { 0.0 };
                assert_close_with(prod[(r, c)], expected, 1e-9);
            }
        }
    }
}

use std::cmp::min;
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::{Bound, Deref, RangeBounds};
use std::sync::Arc;

/// Immutable byte buffer that provides cheap clones and slicing without
/// depending on the third-party `bytes` crate. Internally the data is backed by
/// an `Arc<[u8]>` so multiple clones can reference the same allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bytes {
    inner: Arc<Vec<u8>>,
    start: usize,
    end: usize,
}

impl Bytes {
    /// Construct an empty buffer.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Vec::new()),
            start: 0,
            end: 0,
        }
    }

    /// Construct a buffer from a static slice.
    pub fn from_static(slice: &'static [u8]) -> Self {
        Self {
            inner: Arc::new(slice.to_vec()),
            start: 0,
            end: slice.len(),
        }
    }

    /// Number of bytes in the view.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns `true` when the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return the backing slice of the view.
    pub fn as_slice(&self) -> &[u8] {
        &self.inner[self.start..self.end]
    }

    /// Return a mutable slice when the buffer is uniquely owned.
    pub fn as_mut_slice(&mut self) -> Option<&mut [u8]> {
        if self.start != 0 || self.end != self.inner.len() {
            return None;
        }

        Arc::get_mut(&mut self.inner).map(|vec| vec.as_mut_slice())
    }

    /// Copy the bytes into an owned `Vec<u8>`.
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }

    /// Try to convert into a `Vec<u8>` without copying when the buffer is the
    /// sole owner of the allocation.
    pub fn into_vec(mut self) -> Vec<u8> {
        if self.start == 0 && self.end == self.inner.len() {
            if let Some(vec) = Arc::get_mut(&mut self.inner) {
                return mem::take(vec);
            }
        }
        self.as_slice().to_vec()
    }

    /// Create a new view over the same allocation restricted to the provided
    /// range.
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let len = self.len();

        let start = match range.start_bound() {
            Bound::Included(&idx) => idx,
            Bound::Excluded(&idx) => idx.saturating_add(1),
            Bound::Unbounded => 0,
        };

        let mut end = match range.end_bound() {
            Bound::Included(&idx) => idx.saturating_add(1),
            Bound::Excluded(&idx) => idx,
            Bound::Unbounded => len,
        };

        let start = min(start, len);
        end = min(end, len);
        assert!(start <= end, "invalid slice range");

        Self {
            inner: Arc::clone(&self.inner),
            start: self.start + start,
            end: self.start + end,
        }
    }

    /// Split off the first `at` bytes into a new `Bytes`, updating `self` to
    /// start at the split point.
    pub fn split_to(&mut self, at: usize) -> Self {
        assert!(at <= self.len(), "split position out of bounds");
        let split_end = self.start + at;
        let head = Self {
            inner: Arc::clone(&self.inner),
            start: self.start,
            end: split_end,
        };
        self.start = split_end;
        head
    }

    /// Split off the tail of the buffer starting at `at`.
    pub fn split_off(&mut self, at: usize) -> Self {
        assert!(at <= self.len(), "split position out of bounds");
        let new_start = self.start + at;
        let tail = Self {
            inner: Arc::clone(&self.inner),
            start: new_start,
            end: self.end,
        };
        self.end = new_start;
        tail
    }
}

impl Default for Bytes {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(vec: Vec<u8>) -> Self {
        let len = vec.len();
        Self {
            inner: Arc::new(vec),
            start: 0,
            end: len,
        }
    }
}

impl Hash for Bytes {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl From<&[u8]> for Bytes {
    fn from(slice: &[u8]) -> Self {
        Self::from(slice.to_vec())
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

/// Growable byte buffer. Unlike `Bytes` this type owns a mutable `Vec<u8>` that
/// can be appended to before being frozen into an immutable `Bytes` view.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct BytesMut {
    inner: Vec<u8>,
}

impl BytesMut {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Create a buffer with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }

    /// Return the current length.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Return the current capacity.
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Returns `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Reserve additional capacity.
    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    /// Shrink the allocation to fit the current length.
    pub fn shrink_to_fit(&mut self) {
        self.inner.shrink_to_fit();
    }

    /// Resize the buffer to `len`, filling new elements with `value`.
    pub fn resize(&mut self, len: usize, value: u8) {
        self.inner.resize(len, value);
    }

    /// Extend the buffer from the provided slice.
    pub fn extend_from_slice(&mut self, data: &[u8]) {
        self.inner.extend_from_slice(data);
    }

    /// Push a single byte onto the buffer.
    pub fn push(&mut self, byte: u8) {
        self.inner.push(byte);
    }

    /// Pop a single byte from the end of the buffer.
    pub fn pop(&mut self) -> Option<u8> {
        self.inner.pop()
    }

    /// Clear the buffer without releasing the allocation.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Truncate the buffer to the provided length.
    pub fn truncate(&mut self, len: usize) {
        self.inner.truncate(len);
    }

    /// Split off the first `at` bytes, leaving the remainder in `self`.
    pub fn split_to(&mut self, at: usize) -> Self {
        assert!(at <= self.len(), "split position out of bounds");
        let tail = self.inner.split_off(at);
        let head = std::mem::replace(&mut self.inner, tail);
        Self { inner: head }
    }

    /// Split off the bytes starting at `at`, keeping the prefix in `self`.
    pub fn split_off(&mut self, at: usize) -> Self {
        assert!(at <= self.len(), "split position out of bounds");
        Self {
            inner: self.inner.split_off(at),
        }
    }

    /// Convert into an immutable `Bytes` buffer. This attempts to reuse the
    /// allocation without copying.
    pub fn freeze(self) -> Bytes {
        Bytes::from(self.inner)
    }

    /// Consume the buffer returning the underlying vector.
    pub fn into_vec(self) -> Vec<u8> {
        self.inner
    }

    /// View the buffer as an immutable slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.inner
    }

    /// View the buffer as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.inner
    }
}

impl From<Vec<u8>> for BytesMut {
    fn from(vec: Vec<u8>) -> Self {
        Self { inner: vec }
    }
}

impl From<Bytes> for BytesMut {
    fn from(bytes: Bytes) -> Self {
        Self {
            inner: bytes.to_vec(),
        }
    }
}

impl AsRef<[u8]> for BytesMut {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for BytesMut {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl Deref for BytesMut {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl From<BytesMut> for Bytes {
    fn from(buf: BytesMut) -> Self {
        buf.freeze()
    }
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::Bytes;
    use serde::de::Visitor;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::fmt;

    impl Serialize for Bytes {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_bytes(self.as_ref())
        }
    }

    struct BytesVisitor;

    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = Bytes;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte buffer")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Bytes::from(v))
        }

        fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Bytes::from(v))
        }
    }

    impl<'de> Deserialize<'de> for Bytes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_byte_buf(BytesVisitor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Bytes, BytesMut};

    #[test]
    fn bytes_clone_and_slice_share_allocation() {
        let original = Bytes::from(vec![1, 2, 3, 4]);
        let mut split = original.clone();
        let head = split.split_to(2);
        assert_eq!(head.as_ref(), &[1, 2]);
        assert_eq!(split.as_ref(), &[3, 4]);
        assert_eq!(original.as_ref(), &[1, 2, 3, 4]);
    }

    #[test]
    fn bytes_mut_split_and_freeze() {
        let mut buf = BytesMut::from(vec![10, 20, 30, 40]);
        let head = buf.split_to(2);
        assert_eq!(head.as_ref(), &[10, 20]);
        assert_eq!(buf.as_ref(), &[30, 40]);

        let frozen = buf.freeze();
        assert_eq!(frozen.as_ref(), &[30, 40]);
    }

    #[test]
    fn bytes_into_vec_reuses_allocation() {
        let bytes = Bytes::from(vec![5, 6, 7]);
        let vec = bytes.into_vec();
        assert_eq!(vec, vec![5, 6, 7]);
    }
}

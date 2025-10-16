#![deny(unsafe_op_in_unsafe_fn)]

use std::convert::TryFrom;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::RangeInclusive;

/// Result type returned by fuzz data helpers.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced when decoding fuzz input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The provided data does not contain enough bytes to satisfy the request.
    NotEnoughData,
    /// The requested range is invalid or empty.
    InvalidRange,
    /// Arithmetic overflow occurred while computing bounds.
    Overflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotEnoughData => write!(f, "not enough fuzz input to satisfy request"),
            Error::InvalidRange => write!(f, "requested range is invalid"),
            Error::Overflow => write!(f, "range calculation overflowed"),
        }
    }
}

impl std::error::Error for Error {}

/// Cursor over raw fuzzer-provided bytes with helpers to consume structured data.
#[derive(Clone, Copy)]
pub struct Unstructured<'a> {
    data: &'a [u8],
    cursor: usize,
}

impl<'a> Unstructured<'a> {
    /// Construct a cursor over the provided data slice.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, cursor: 0 }
    }

    /// Number of bytes still available for consumption.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.cursor)
    }

    /// Returns `true` when no bytes remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Attempt to consume the requested number of bytes without copying.
    fn take_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.cursor.checked_add(len).ok_or(Error::Overflow)?;
        if end > self.data.len() {
            return Err(Error::NotEnoughData);
        }
        let slice = &self.data[self.cursor..end];
        self.cursor = end;
        Ok(slice)
    }

    /// Fill the destination buffer with bytes from the input.
    pub fn fill_buffer(&mut self, dest: &mut [u8]) -> Result<()> {
        let bytes = self.take_bytes(dest.len())?;
        dest.copy_from_slice(bytes);
        Ok(())
    }

    /// Draw an integer within the inclusive range using rejection-style sampling.
    pub fn int_in_range(&mut self, range: RangeInclusive<u64>) -> Result<u64> {
        let start = *range.start();
        let end = *range.end();
        if start > end {
            return Err(Error::InvalidRange);
        }
        let span = end
            .checked_sub(start)
            .and_then(|delta| delta.checked_add(1))
            .ok_or(Error::Overflow)?;
        let raw = self.read_u64()?;
        if span == 0 {
            return Err(Error::Overflow);
        }
        let value = if span.is_power_of_two() {
            raw & (span - 1)
        } else {
            raw % span
        };
        Ok(start + value)
    }

    /// Decode a value implementing [`Arbitrary`].
    pub fn arbitrary<T>(&mut self) -> Result<T>
    where
        T: Arbitrary<'a>,
    {
        T::arbitrary(self)
    }

    fn read_u64(&mut self) -> Result<u64> {
        let mut bytes = [0u8; 8];
        self.fill_buffer(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Decode an [`IpAddr`] from the input, returning either IPv4 or IPv6 based on a flag.
    pub fn ip_addr(&mut self) -> Result<IpAddr> {
        if self.is_empty() {
            return Err(Error::NotEnoughData);
        }
        let version_flag = self.arbitrary::<bool>()?;
        if version_flag {
            let mut bytes = [0u8; 16];
            self.fill_buffer(&mut bytes)?;
            Ok(IpAddr::V6(Ipv6Addr::from(bytes)))
        } else {
            let mut bytes = [0u8; 4];
            self.fill_buffer(&mut bytes)?;
            Ok(IpAddr::V4(Ipv4Addr::from(bytes)))
        }
    }
}

/// Trait implemented by types that can be constructed from fuzz input.
pub trait Arbitrary<'a>: Sized {
    /// Attempt to construct `Self` from the provided cursor.
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self>;
}

impl<'a> Arbitrary<'a> for bool {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        Ok(u.arbitrary::<u8>()? & 1 == 1)
    }
}

impl<'a> Arbitrary<'a> for u8 {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let bytes = u.take_bytes(1)?;
        Ok(bytes[0])
    }
}

impl<'a> Arbitrary<'a> for u16 {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let mut bytes = [0u8; 2];
        u.fill_buffer(&mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }
}

impl<'a> Arbitrary<'a> for u32 {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let mut bytes = [0u8; 4];
        u.fill_buffer(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }
}

impl<'a> Arbitrary<'a> for u64 {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let mut bytes = [0u8; 8];
        u.fill_buffer(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }
}

impl<'a> Arbitrary<'a> for usize {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let value = u.arbitrary::<u64>()?;
        usize::try_from(value).map_err(|_| Error::Overflow)
    }
}

impl<'a, const N: usize> Arbitrary<'a> for [u8; N] {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let mut buf = [0u8; N];
        u.fill_buffer(&mut buf)?;
        Ok(buf)
    }
}

impl<'a> Arbitrary<'a> for Vec<u8> {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let remaining = u.remaining() as u64;
        let len = if remaining == 0 {
            0
        } else {
            u.int_in_range(0..=remaining)? as usize
        };
        let bytes = u.take_bytes(len)?;
        Ok(bytes.to_vec())
    }
}

impl<'a, T> Arbitrary<'a> for Option<T>
where
    T: Arbitrary<'a>,
{
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        if u.is_empty() {
            return Ok(None);
        }
        let flag = u.arbitrary::<bool>()?;
        if flag {
            Ok(Some(T::arbitrary(u)?))
        } else {
            Ok(None)
        }
    }
}

/// Internal helpers for the exported fuzz macros.
#[doc(hidden)]
pub mod harness {
    use super::Result;
    use std::ffi::{c_char, c_int};
    use std::slice;

    /// Invoke the provided closure with the raw fuzz bytes. The caller guarantees
    /// that the pointer is valid for `len` bytes for the duration of the call.
    pub fn with_bytes<F>(data: *const u8, len: usize, mut f: F)
    where
        F: FnMut(&[u8]),
    {
        if len == 0 || data.is_null() {
            f(&[]);
            return;
        }
        // Safety: libFuzzer promises that the pointer is valid for `len` bytes
        // for the duration of the call.
        let bytes = unsafe { slice::from_raw_parts(data, len) };
        f(bytes);
    }

    /// Signature compatible with libFuzzer's optional `LLVMFuzzerInitialize`.
    #[no_mangle]
    pub extern "C" fn LLVMFuzzerInitialize(_argc: *mut c_int, _argv: *mut *mut c_char) -> c_int {
        0
    }

    /// Helper used by the macro to decode typed input.
    pub fn with_arbitrary<T, F>(data: *const u8, len: usize, mut f: F)
    where
        for<'a> T: crate::Arbitrary<'a>,
        F: FnMut(Result<T>),
    {
        with_bytes(data, len, |bytes| {
            let mut unstructured = crate::Unstructured::new(bytes);
            let value = T::arbitrary(&mut unstructured);
            f(value);
        });
    }
}

/// Macro-compatible libFuzzer entrypoint.
#[macro_export]
macro_rules! fuzz_target {
    (|$data:ident : &[u8]| $body:block) => {
        #[no_mangle]
        pub extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, len: usize) -> i32 {
            $crate::harness::with_bytes(data, len, |$data| $body);
            0
        }
    };
    (|$value:ident : $ty:ty| $body:block) => {
        #[no_mangle]
        pub extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, len: usize) -> i32 {
            $crate::harness::with_arbitrary::<$ty, _>(data, len, |res| {
                if let Ok($value) = res {
                    $body
                }
            });
            0
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_within_bounds() {
        let data = [0xff; 16];
        let mut u = Unstructured::new(&data);
        let value = u.int_in_range(0..=15).unwrap();
        assert!(value <= 15);
    }

    #[test]
    fn arbitrary_vec_respects_remaining() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut u = Unstructured::new(&data);
        let _ = u.arbitrary::<u64>().unwrap();
        let mut u = Unstructured::new(&data);
        let vec = u.arbitrary::<Vec<u8>>().unwrap();
        assert!(vec.len() <= data.len());
    }

    #[test]
    fn ip_addr_decodes_ipv4() {
        let bytes = [0u8, 192, 168, 1, 42];
        let mut cursor = Unstructured::new(&bytes);
        let addr = cursor.ip_addr().unwrap();
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42)));
        assert!(cursor.is_empty());
    }

    #[test]
    fn ip_addr_decodes_ipv6() {
        let mut bytes = [0u8; 17];
        bytes[0] = 1;
        bytes[1..].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let mut cursor = Unstructured::new(&bytes);
        let addr = cursor.ip_addr().unwrap();
        assert_eq!(
            addr,
            IpAddr::V6(Ipv6Addr::new(
                0x0102, 0x0304, 0x0506, 0x0708, 0x090a, 0x0b0c, 0x0d0e, 0x0f10
            ))
        );
        assert!(cursor.is_empty());
    }

    #[test]
    fn ip_addr_errors_on_short_input() {
        let bytes = [0u8, 1, 2];
        let mut cursor = Unstructured::new(&bytes);
        assert_eq!(cursor.ip_addr(), Err(Error::NotEnoughData));
    }
}

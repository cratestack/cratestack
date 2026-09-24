//! A strict, allocation-free CBOR reader for the few item kinds a COSE
//! message may contain here.
//!
//! "Strict" means it accepts only what [`super::write`] emits: definite
//! lengths, and the shortest head for every argument. Indefinite lengths
//! (additional information 31) and the reserved values 28-30 are rejected,
//! and so is a head that could have been shorter (`0x18 0x05` for 5). Two
//! encodings of one message would be two byte strings that verify the same
//! way, and `request_digest` hashes the bytes (§4), so one message has
//! exactly one encoding.
//!
//! Every length is checked against the remaining input before it is used,
//! so a hostile head (`0x5b` with a 2⁶⁴-1 length) is a clean [`Reject`],
//! never an allocation or a panic.

use std::ops::Range;

use super::write::{MAJOR_BSTR, MAJOR_NINT, MAJOR_UINT};
use crate::error::Reject;

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.pos == self.buf.len()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], Reject> {
        let end = self.pos.checked_add(len).ok_or(Reject)?;
        let bytes = self.buf.get(self.pos..end).ok_or(Reject)?;
        self.pos = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8, Reject> {
        Ok(self.take(1)?[0])
    }

    /// Consume one exact byte, e.g. the empty unprotected map `0xa0`.
    pub(crate) fn expect_byte(&mut self, expected: u8) -> Result<(), Reject> {
        if self.byte()? == expected {
            Ok(())
        } else {
            Err(Reject)
        }
    }

    /// Read a head and return `(major, argument)`. Meaningful for majors 0
    /// to 6 only; callers never accept major 7 (floats and simple values)
    /// through it.
    fn head(&mut self) -> Result<(u8, u64), Reject> {
        let initial = self.byte()?;
        let major = initial >> 5;
        let info = initial & 0x1f;
        let arg = match info {
            0..=23 => u64::from(info),
            24 => Self::minimal(u64::from(self.byte()?), 24)?,
            25 => Self::minimal(u64::from(u16::from_be_bytes(self.array()?)), 0x100)?,
            26 => Self::minimal(u64::from(u32::from_be_bytes(self.array()?)), 0x1_0000)?,
            27 => Self::minimal(u64::from_be_bytes(self.array()?), 0x1_0000_0000)?,
            // 28-30 are reserved; 31 is an indefinite length or "break".
            _ => return Err(Reject),
        };
        Ok((major, arg))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Reject> {
        self.take(N)?.try_into().map_err(|_| Reject)
    }

    /// Reject an argument that a shorter head could have carried.
    fn minimal(arg: u64, floor: u64) -> Result<u64, Reject> {
        if arg >= floor { Ok(arg) } else { Err(Reject) }
    }

    /// Read a head of the given major type and return its argument.
    pub(crate) fn expect(&mut self, major: u8) -> Result<u64, Reject> {
        match self.head()? {
            (found, arg) if found == major => Ok(arg),
            _ => Err(Reject),
        }
    }

    /// A definite-length byte string, as a range into the input, so the
    /// caller can slice the original `Bytes` without copying.
    pub(crate) fn bstr_range(&mut self) -> Result<Range<usize>, Reject> {
        let len = usize::try_from(self.expect(MAJOR_BSTR)?).map_err(|_| Reject)?;
        let start = self.pos;
        self.take(len)?;
        Ok(start..self.pos)
    }

    pub(crate) fn uint(&mut self) -> Result<u64, Reject> {
        self.expect(MAJOR_UINT)
    }

    /// A major-0 or major-1 integer that fits an `i64`.
    pub(crate) fn int(&mut self) -> Result<i64, Reject> {
        match self.head()? {
            (MAJOR_UINT, arg) => i64::try_from(arg).map_err(|_| Reject),
            (MAJOR_NINT, arg) => Ok(-1 - i64::try_from(arg).map_err(|_| Reject)?),
            _ => Err(Reject),
        }
    }
}

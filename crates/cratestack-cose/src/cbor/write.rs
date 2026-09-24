//! Deterministic CBOR emission (RFC 8949 §4.2.1): definite lengths and the
//! shortest head for every argument. Only the handful of item kinds the
//! envelope writes are here.

pub(crate) const MAJOR_UINT: u8 = 0;
pub(crate) const MAJOR_NINT: u8 = 1;
pub(crate) const MAJOR_BSTR: u8 = 2;
pub(crate) const MAJOR_TSTR: u8 = 3;
pub(crate) const MAJOR_ARRAY: u8 = 4;
pub(crate) const MAJOR_MAP: u8 = 5;
pub(crate) const MAJOR_TAG: u8 = 6;
/// `null` (major 7, simple value 22).
pub(crate) const NULL: u8 = 0xf6;
/// The empty map, the only unprotected header this crate writes or reads.
pub(crate) const EMPTY_MAP: u8 = 0xa0;

/// Write the shortest head for `major` / `arg`.
pub(crate) fn head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let initial = major << 5;
    if arg < 24 {
        out.push(initial | arg as u8);
    } else if let Ok(byte) = u8::try_from(arg) {
        out.extend_from_slice(&[initial | 24, byte]);
    } else if let Ok(short) = u16::try_from(arg) {
        out.push(initial | 25);
        out.extend_from_slice(&short.to_be_bytes());
    } else if let Ok(word) = u32::try_from(arg) {
        out.push(initial | 26);
        out.extend_from_slice(&word.to_be_bytes());
    } else {
        out.push(initial | 27);
        out.extend_from_slice(&arg.to_be_bytes());
    }
}

/// The length of the head [`head`] writes for `arg`, so buffers can be
/// sized exactly before anything is written.
pub(crate) const fn head_len(arg: u64) -> usize {
    if arg < 24 {
        1
    } else if arg <= 0xff {
        2
    } else if arg <= 0xffff {
        3
    } else if arg <= 0xffff_ffff {
        5
    } else {
        9
    }
}

/// `usize` to a CBOR argument. Infallible on every target Rust supports
/// (`usize` is at most 64 bits), spelled out so no `as` cast can truncate.
pub(crate) fn len_arg(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

pub(crate) fn bstr(out: &mut Vec<u8>, bytes: &[u8]) {
    head(out, MAJOR_BSTR, len_arg(bytes.len()));
    out.extend_from_slice(bytes);
}

/// The encoded length of a `bstr` holding `len` bytes.
pub(crate) fn bstr_len(len: usize) -> usize {
    head_len(len_arg(len)) + len
}

pub(crate) fn tstr(out: &mut Vec<u8>, text: &str) {
    head(out, MAJOR_TSTR, len_arg(text.len()));
    out.extend_from_slice(text.as_bytes());
}

pub(crate) fn int(out: &mut Vec<u8>, value: i64) {
    match u64::try_from(value) {
        Ok(unsigned) => head(out, MAJOR_UINT, unsigned),
        // `-1 - value` is in `0..=i64::MAX` for every negative `value`, so
        // neither the subtraction nor the conversion can fail.
        Err(_) => head(out, MAJOR_NINT, u64::try_from(-1 - value).unwrap_or(0)),
    }
}

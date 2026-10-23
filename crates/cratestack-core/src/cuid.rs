//! Generate the value for `@default(cuid())` (cratestack#1209).
//!
//! The id has cuid2's default shape: 24 characters, a lowercase letter
//! first, then lowercase letters and digits. That is what the `Cuid`
//! scalar accepts ([`crate::error::parse_cuid`]) and what other cuid2
//! producers emit, so ids from either source mix in one column.
//!
//! It is drawn straight from a CSPRNG rather than through the cuid2
//! algorithm's hash of time, counter and host fingerprint. That keeps
//! the property cuid2 exists for (ids that cannot be guessed and do not
//! collide: about 124 random bits) without its dependencies. The `cuid2`
//! crate needs `getrandom` 0.3, which does not build for
//! `wasm32-unknown-unknown` without extra configuration, and the embedded
//! runtime runs there. The randomness comes from `uuid`'s v4 generator,
//! which every runtime already depends on and which the embedded runtime
//! already wires to the browser's `crypto.getRandomValues` on wasm.

const LENGTH: usize = 24;
const LETTERS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// A new random id in cuid2's default shape (`[a-z][0-9a-z]{23}`).
pub fn new_cuid() -> String {
    // 26 random bytes (208 bits): two v4 UUIDs minus the bytes that carry
    // their version and variant bits.
    let mut number = Vec::with_capacity(26);
    for _ in 0..2 {
        let bytes = *uuid::Uuid::new_v4().as_bytes();
        number.extend_from_slice(&bytes[..6]);
        number.extend_from_slice(&bytes[9..]);
    }
    let mut id = String::with_capacity(LENGTH);
    id.push(LETTERS[divide(&mut number, 26)] as char);
    for _ in 1..LENGTH {
        id.push(ALPHABET[divide(&mut number, 36)] as char);
    }
    id
}

/// Divide the big-endian number in place and return the remainder. The
/// ~124 bits taken from 208 leave the digits uniform to within 2^-84.
fn divide(number: &mut [u8], divisor: u32) -> usize {
    let mut remainder = 0u32;
    for byte in number.iter_mut() {
        let value = (remainder << 8) | u32::from(*byte);
        *byte = (value / divisor) as u8;
        remainder = value % divisor;
    }
    remainder as usize
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::new_cuid;
    use crate::error::parse_cuid;

    #[test]
    fn ids_have_the_cuid2_shape_and_parse_as_cuid() {
        for _ in 0..1000 {
            let id = new_cuid();
            assert_eq!(id.len(), 24, "{id}");
            assert!(id.as_bytes()[0].is_ascii_lowercase(), "{id}");
            assert!(parse_cuid(&id).is_ok(), "{id}");
        }
    }

    #[test]
    fn ids_do_not_repeat_and_use_the_whole_alphabet() {
        let ids: HashSet<String> = (0..10_000).map(|_| new_cuid()).collect();
        assert_eq!(ids.len(), 10_000);
        let seen: HashSet<char> = ids.iter().flat_map(|id| id.chars().skip(1)).collect();
        assert_eq!(seen.len(), 36, "every base36 digit appears");
    }
}

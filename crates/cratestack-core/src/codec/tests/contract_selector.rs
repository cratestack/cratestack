//! The `Cratestack-Contract` selector: 8 bytes of the op digest, strictly
//! parsed, one spelling each.

use crate::codec::{
    CONTRACT_HEADER, CONTRACT_HEADER_VALUE_LEN, CONTRACT_SELECTOR_LEN, ContractSelector,
};
use crate::error::CratestackError;

fn digest() -> [u8; 32] {
    std::array::from_fn(|i| i as u8)
}

#[test]
fn the_selector_is_the_digests_first_eight_bytes() {
    let selector = ContractSelector::of(&digest());
    assert!(selector.matches(&digest()));
    let mut other = digest();
    other[CONTRACT_SELECTOR_LEN - 1] ^= 1;
    assert!(!selector.matches(&other));
    // Only the prefix is compared: this is a selector, not an identity.
    other = digest();
    other[CONTRACT_SELECTOR_LEN] ^= 1;
    assert!(selector.matches(&other));
}

#[test]
fn the_header_value_is_eleven_characters_of_unpadded_base64url() {
    let value = ContractSelector::of(&digest()).to_header_value();
    assert_eq!(value, "AAECAwQFBgc");
    assert_eq!(value.len(), CONTRACT_HEADER_VALUE_LEN);
    assert_eq!(CONTRACT_HEADER, "Cratestack-Contract");
}

#[test]
fn a_value_round_trips() {
    let selector = ContractSelector::of(&digest());
    let parsed = ContractSelector::from_header_value(b"AAECAwQFBgc").unwrap();
    assert_eq!(parsed, selector);
}

#[test]
fn a_malformed_value_is_a_bad_request() {
    for value in [
        &b""[..],
        b"AAECAwQFBg",     // 10 characters
        b"AAECAwQFBgcA",   // 12 characters
        b"AAECAwQFBgc=",   // padded
        b"AAECAwQFBg+",    // standard-alphabet character
        b"AAECAwQFBgd",    // non-zero trailing bits: a second spelling
        b"AAECAwQFBg\xff", // not ASCII
    ] {
        let error = ContractSelector::from_header_value(value).unwrap_err();
        assert!(
            matches!(error, CratestackError::BadRequest(_)),
            "{value:?}: {error:?}"
        );
    }
}

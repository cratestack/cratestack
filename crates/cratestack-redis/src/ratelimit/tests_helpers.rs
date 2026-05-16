#![cfg(test)]

use std::time::{Duration, UNIX_EPOCH};

use super::time::system_time_to_ms;
use super::util::nibble_hex;

#[test]
fn nibble_hex_covers_all_valid_nibbles() {
    let expected = "0123456789abcdef";
    for (n, ch) in expected.chars().enumerate() {
        assert_eq!(nibble_hex(n as u8), ch);
    }
}

#[test]
fn system_time_to_ms_rejects_pre_epoch_inputs() {
    let before = UNIX_EPOCH - Duration::from_secs(1);
    assert!(system_time_to_ms(before).is_err());
}

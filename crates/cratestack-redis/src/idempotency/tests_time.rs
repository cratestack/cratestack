#![cfg(test)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_core::CoolError;

use super::time::{system_time_from_ms, system_time_to_ms};

#[test]
fn system_time_ms_roundtrip_near_now() {
    let now = SystemTime::now();
    let ms = system_time_to_ms(now).expect("to ms");
    let back = system_time_from_ms(ms);
    let drift = now
        .duration_since(back)
        .or_else(|err| Ok::<_, std::time::SystemTimeError>(err.duration()))
        .unwrap();
    assert!(drift < Duration::from_millis(2), "roundtrip drift: {drift:?}");
}

#[test]
fn system_time_from_ms_handles_negative_input() {
    // Negative `expires_at` shouldn't panic — Redis stores
    // arbitrary i64 values, and a buggy upstream could surface
    // one. We document the conversion as best-effort here.
    let result = system_time_from_ms(-1_000);
    assert!(result < UNIX_EPOCH);
}

#[test]
fn system_time_to_ms_rejects_pre_epoch_inputs() {
    let before = UNIX_EPOCH - Duration::from_secs(1);
    let err = system_time_to_ms(before).expect_err("pre-epoch must error");
    assert!(matches!(err, CoolError::Internal(_)));
}

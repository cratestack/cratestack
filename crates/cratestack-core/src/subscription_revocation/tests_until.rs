use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;

fn key() -> RevocationKey {
    RevocationKey::Session("s".to_owned())
}

fn payload(until: &str) -> String {
    format!(r#"{{"kind":"session","key":"s","reason":"session_ended","until":{until}}}"#)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The tombstone `key()` holds after applying `until`, if any.
fn applied(until: &str) -> (Option<SystemTime>, usize) {
    let revocations = StreamRevocations::default();
    let watch = revocations.watch(vec![key()]).unwrap();
    revocations.apply(&payload(until));
    assert!(watch.try_ended().is_some(), "{until} must still revoke");
    let state = revocations.state.lock().unwrap();
    (
        state.tombstones.get(&key()).map(|t| t.until),
        state.tombstones.len(),
    )
}

#[test]
fn a_fractional_until_is_rounded_up_not_clamped() {
    let secs = now_secs() + 30 * 24 * 3600;
    let (until, _) = applied(&format!("{secs}.123"));
    assert_eq!(until, Some(UNIX_EPOCH + Duration::from_secs(secs + 1)));
}

#[test]
fn a_numeric_string_until_is_read() {
    let secs = now_secs() + 30 * 24 * 3600;
    let (until, _) = applied(&format!("\"{secs}.5\""));
    assert_eq!(until, Some(UNIX_EPOCH + Duration::from_secs(secs + 1)));
    let (until, _) = applied(&format!("\"{secs}\""));
    assert_eq!(until, Some(UNIX_EPOCH + Duration::from_secs(secs)));
}

#[test]
fn an_unrepresentable_until_is_clamped_and_still_revokes() {
    for until in ["18446744073709551615", "1e30"] {
        let (until, _) = applied(until);
        let until = until.expect("clamped, so still tombstoned");
        let ahead = until.duration_since(SystemTime::now()).unwrap();
        assert!(ahead <= Duration::from_secs(24 * 3600), "{ahead:?}");
    }
}

#[test]
fn a_malformed_until_revokes_without_a_tombstone() {
    for until in [
        "-5", "-0.5", "\"NaN\"", "\"inf\"", "\"soon\"", "true", "[1]",
    ] {
        assert_eq!(applied(until), (None, 0), "{until}");
    }
}

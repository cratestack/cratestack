//! Capacity behavior only: pins deliberately live no longer than this state.

use super::{CompactClient, MAX_REMEMBERED_KEYS, lock};
use cratestack_core::compact::KeyTables;

fn state() -> CompactClient {
    CompactClient::new(
        KeyTables {
            types: vec![],
            ops: vec![],
        },
        None,
    )
}

#[test]
fn saturation_preserves_named_and_compact_pins_and_bounds_storage() {
    let state = state();
    assert!(!state.decide("op", Some("named")));
    state.set_compact("op", true);
    assert!(state.decide("op", Some("compact")));
    for number in 2..MAX_REMEMBERED_KEYS {
        assert!(state.decide("op", Some(&format!("key-{number}"))));
    }
    assert!(!state.decide("op", Some("named")));
    assert!(state.decide("op", Some("compact")));
    for number in 0..MAX_REMEMBERED_KEYS * 2 {
        assert!(!state.decide("op", Some(&format!("overflow-{number}"))));
    }
    assert_eq!(lock(&state.idempotent).len(), MAX_REMEMBERED_KEYS);
    assert!(state.decide("op", Some("compact")));
    assert!(!state.decide("op", Some("named")));
    assert!(state.decide("op", None), "unkeyed calls still compact");
}

#[test]
fn overflow_remains_named_across_negotiation_changes() {
    let state = state();
    state.set_compact("op", true);
    for number in 0..MAX_REMEMBERED_KEYS {
        assert!(state.decide("op", Some(&format!("key-{number}"))));
    }
    assert!(!state.decide("op", Some("overflow")));
    state.set_compact("op", false);
    assert!(!state.decide("op", Some("overflow")));
    assert!(
        state.decide("op", Some("key-0")),
        "old compact pin survives downgrade"
    );
    state.set_compact("op", true);
    assert!(!state.decide("op", Some("overflow")));
}

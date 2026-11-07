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
fn a_retry_reuses_its_first_encoding_across_a_negotiation_change() {
    let state = state();
    assert!(!state.decide("op", Some("named")));
    state.set_compact("op", true);
    assert!(state.decide("op", Some("compact")));
    assert!(
        !state.decide("op", Some("named")),
        "named pin survives upgrade"
    );
    state.set_compact("op", false);
    assert!(
        state.decide("op", Some("compact")),
        "compact pin survives downgrade"
    );
    assert!(
        !state.decide("op", None),
        "unkeyed calls follow negotiation"
    );
}

#[test]
fn a_full_table_evicts_the_oldest_pin_and_keeps_negotiating() {
    let state = state();
    assert!(!state.decide("op", Some("key-0")));
    state.set_compact("op", true);
    for number in 1..MAX_REMEMBERED_KEYS {
        assert!(state.decide("op", Some(&format!("key-{number}"))));
    }
    // A new key after the window still follows negotiation, and is pinned.
    assert!(state.decide("op", Some("fresh")));
    state.set_compact("op", false);
    assert!(state.decide("op", Some("fresh")), "recent pin retained");
    let last = format!("key-{}", MAX_REMEMBERED_KEYS - 1);
    assert!(state.decide("op", Some(&last)), "recent pin retained");
    // The oldest pin (key-0, named) was evicted: it now follows negotiation.
    state.set_compact("op", true);
    assert!(state.decide("op", Some("key-0")));
    let pins = lock(&state.idempotent);
    assert_eq!(pins.by_key.len(), MAX_REMEMBERED_KEYS);
}

//! A lock of a format this build does not know says so (review of
//! cratestack#1132, N4), rather than "unknown field" for a key the newer
//! format added.

use super::{ContractLock, LockError};

#[test]
fn a_newer_format_with_a_new_key_reports_its_format() {
    let text =
        r#"{"format": 2, "domain": "x", "contracts": {}, "generations": [], "added_in_2": true}"#;
    match ContractLock::parse(text) {
        Err(LockError::Format(2)) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_with_no_format_is_still_not_valid() {
    assert!(matches!(
        ContractLock::parse(r#"{"domain": "x"}"#),
        Err(LockError::Parse(_))
    ));
}

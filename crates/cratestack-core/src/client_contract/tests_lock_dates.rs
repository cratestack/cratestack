//! The lock's dates are real `YYYY-MM-DD` dates, compared as dates (review
//! of cratestack#1132, S3): a text comparison of `10/02/2026` against
//! `2026-01-01` would prune a generation locked yesterday.

use super::tests_compat::base;
use super::{ContractLock, LockError};

fn lock_on(date: &str) -> ContractLock {
    let mut lock = ContractLock::new();
    lock.lock_generation(&base(), date, "").unwrap();
    lock
}

#[test]
fn only_real_calendar_dates_are_accepted() {
    for ok in ["2026-10-02", "2024-02-29", "2000-02-29", "0001-01-01"] {
        assert!(lock_on_ok(ok), "{ok}");
    }
    for bad in [
        "10/02/2026",
        "2026-1-2",
        "2026-13-01",
        "2026-00-10",
        "2026-04-31",
        "2025-02-29",
        "1900-02-29",
        "2026-10-00",
        "2026-10-32",
        "2026-10-02 ",
        "+026-10-02",
        "",
        "yesterday",
        "２０２６-10-02",
    ] {
        assert!(!lock_on_ok(bad), "{bad:?}");
    }
}

fn lock_on_ok(date: &str) -> bool {
    match ContractLock::new().lock_generation(&base(), date, "") {
        Ok(_) => true,
        Err(LockError::Date(d)) => {
            assert_eq!(d, date);
            false
        }
        Err(other) => panic!("{other}"),
    }
}

#[test]
fn a_bad_lock_date_changes_nothing() {
    let mut lock = ContractLock::new();
    assert!(lock.lock_generation(&base(), "10/02/2026", "").is_err());
    assert!(lock.generations.is_empty() && lock.contracts.is_empty());
}

#[test]
fn prune_before_refuses_a_bad_date_and_removes_nothing() {
    let mut lock = lock_on("2026-10-02");
    for bad in ["10/02/2026", "2026-02-30", "soon"] {
        assert!(
            matches!(lock.prune_before(bad), Err(LockError::Date(_))),
            "{bad}"
        );
    }
    assert_eq!(lock.generations.len(), 1);
}

#[test]
fn prune_before_keeps_the_day_itself_and_drops_earlier_ones() {
    let mut lock = lock_on("2026-10-02");
    assert_eq!(lock.prune_before("2026-10-02").unwrap().generations, 0);
    assert_eq!(lock.prune_before("2026-10-03").unwrap().generations, 1);
}

#[test]
fn a_lock_file_with_a_bad_locked_at_is_refused() {
    let good = lock_on("2026-10-02").to_json();
    assert!(ContractLock::parse(&good).is_ok());
    let bad = good.replace("2026-10-02", "10/02/2026");
    match ContractLock::parse(&bad) {
        Err(LockError::Date(date)) => assert_eq!(date, "10/02/2026"),
        other => panic!("{other:?}"),
    }
}

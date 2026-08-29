//! Identifier and expiry generators for the enrolment/token exchanges.
//!
//! Every id is a `cuid2` with a leading `c`, matching the
//! `@paralleldrive/cuid2`-style ids this workspace's downstream consumers
//! already standardise on.

use chrono::{DateTime, Duration, Utc};

#[cfg(test)]
mod tests;

pub fn enrollment_id() -> String {
    new_cuid()
}

pub fn key_id() -> String {
    new_cuid()
}

pub fn user_id() -> String {
    new_cuid()
}

pub fn challenge() -> String {
    new_cuid()
}

pub fn challenge_expiry() -> DateTime<Utc> {
    Utc::now() + Duration::minutes(15)
}

fn new_cuid() -> String {
    format!("c{}", cuid2::create_id())
}

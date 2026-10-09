//! `BigInt` in the SQL value layer (ADR 0019 PR B, package B3).
//!
//! - `create_policies`: the create path's in-process evaluator. `!=` and
//!   `not in` on a `BigInt` must DENY the matching value (risk 2: they used
//!   to be satisfied by a `_ => false` fallthrough and by derived equality
//!   between `BigInt(7)` and `Int(7)`).
//! - `create_defaults`: `@default(auth().x)` into a `BigInt` column.
//! - `read_policies`: the pushed-down form (read, update, delete) and the
//!   preview renderer agree, with no database.
//! - `binds`, `audit_snapshot`: bind arms and the canonical-string audit form.
//! - `pg` (with `pg_support`): the same predicates against a real Postgres
//!   through `authorize_record_action`, `push_scoped_conditions`, UPDATE and
//!   DELETE. Skipped without a database unless `CRATESTACK_REQUIRE_DB` is set.

mod audit_snapshot;
mod binds;
mod create_claims;
mod create_defaults;
mod create_policies;
mod pg;
mod pg_support;
mod read_policies;

/// `i64::MIN`, `-1`, `0`, `2^53 + 1` (the first integer a JS number cannot
/// hold) and `i64::MAX`.
pub(crate) const BOUNDARIES: [i64; 5] = [i64::MIN, -1, 0, 9_007_199_254_740_993, i64::MAX];

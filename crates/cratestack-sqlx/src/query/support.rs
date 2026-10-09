//! Shared QueryBuilder pushers + policy/auth evaluators used by every
//! delegate operation. Split into focused submodules:
//!
//! - [`conditions`]: `WHERE` assembly + per-action authorization
//!   probe.
//! - [`filter`] / [`filter_subkinds`]: filter-expression pushers.
//! - [`policy`] / [`policy_predicate`] / [`policy_relation`]:
//!   allow/deny dispatch + per-predicate emission + relation EXISTS.
//! - [`order`]: ORDER BY / LIMIT / OFFSET helpers.
//! - [`values`]: `push_bind_value`, `auth_value_to_sql`.
//! - [`comparison`]: the three-valued in-process comparisons behind the
//!   create-path evaluator and `auth().x <op> <literal>`.
//! - [`decimal_bind`]: the `SqlValue::Decimal`/`NullDecimal` → `push_bind`
//!   boundary, split out of `values` (cratestack#505 Direction 2).
//! - [`create`]: create-path auth-default + policy evaluation.
//! - [`unique_violation`]: per-item batch write SQLSTATE 23505
//!   classification.
//! - [`version_probe`]: shared `@version` mismatch-vs-policy-denial
//!   disambiguation for the versioned update/delete paths.

mod comparison;
mod conditions;
mod create;
mod create_eval;
mod db;
mod decimal_bind;
mod filter;
mod filter_subkinds;
mod order;
mod policy;
mod policy_predicate;
mod policy_relation;
mod relation_scope;
mod unique_violation;
mod values;
mod version_probe;

pub(crate) use comparison::{
    sql_value_differs_from_literal, sql_value_matches_literal, value_differs_from_auth_literal,
    value_matches_auth_literal,
};
pub(crate) use conditions::{ReadPolicyKind, authorize_record_action, push_scoped_conditions};
/// The create path evaluates predicates in-process rather than pushing
/// them into SQL, so it is a second evaluator `AuthIsSystem` has to be
/// wired through. Exposed under `cfg(test)` so
/// `crate::tests_system_principal_policy` can drive the real function
/// instead of restating its match arms.
#[cfg(test)]
pub(crate) use create::evaluate_input_predicate as evaluate_input_predicate_for_tests;
pub(crate) use create::{apply_create_defaults, evaluate_create_policies};
pub(crate) use db::PolicyDb;
pub(crate) use filter::{push_filter_expr_query, push_filter_query};
pub(crate) use order::push_order_and_paging;
pub(crate) use policy::{push_action_policy_query, push_policy_expr_query};
pub(crate) use unique_violation::classify_unique_violation;
pub(crate) use values::{auth_value_to_sql, claim_type_suffix, find_column_value, push_bind_value};
pub(crate) use version_probe::no_row_error;

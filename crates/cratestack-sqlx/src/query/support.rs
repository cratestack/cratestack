//! Shared QueryBuilder pushers + policy/auth evaluators used by every
//! delegate operation. Split into focused submodules:
//!
//! - [`conditions`]: `WHERE` assembly + per-action authorization
//!   probe.
//! - [`filter`] / [`filter_subkinds`]: filter-expression pushers.
//! - [`policy`] / [`policy_predicate`] / [`policy_relation`]:
//!   allow/deny dispatch + per-predicate emission + relation EXISTS.
//! - [`order`]: ORDER BY / LIMIT / OFFSET helpers.
//! - [`values`]: `push_bind_value`, `auth_value_to_sql`, the
//!   small literal-equality helpers.
//! - [`create`]: create-path auth-default + policy evaluation.

mod conditions;
mod create;
mod create_eval;
mod filter;
mod filter_subkinds;
mod order;
mod policy;
mod policy_predicate;
mod policy_relation;
mod values;

pub(crate) use conditions::{ReadPolicyKind, authorize_record_action, push_scoped_conditions};
pub(crate) use create::{apply_create_defaults, evaluate_create_policies};
pub(crate) use filter::{push_filter_expr_query, push_filter_query};
pub(crate) use order::push_order_and_paging;
pub(crate) use policy::{push_action_policy_query, push_policy_expr_query};
pub(crate) use values::{
    auth_value_to_sql, find_column_value, push_bind_value, sql_value_matches_literal,
    value_matches_auth_literal,
};

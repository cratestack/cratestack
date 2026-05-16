//! Per-procedure + per-model axum handler/route codegen, plus the
//! shared support fns (`parse_model_list_query` etc.) that the
//! handlers depend on.
//!
//! Split into focused submodules:
//! - [`procedure`]: per-procedure handler + route + `@api_version` /
//!   `@deprecated` attribute helpers.
//! - [`shared_support`]: schema-independent helper fns spliced once
//!   per module.
//! - [`model`]: per-model 5-handler emission (list/create/get/
//!   update/delete) + routes.
//! - [`filter_arms`]: per-scalar-field match arms for the generated
//!   filter/order builders.
//! - [`policy_attr`]: `@@allow(...)` attribute parsing for the
//!   create-handler auth preflight.

mod filter_arms;
mod model;
mod policy_attr;
mod procedure;
mod shared_support;

pub(crate) use model::{generate_model_axum_handlers, generate_model_axum_routes};
pub(crate) use procedure::{generate_procedure_axum_handler, generate_procedure_axum_route};
pub(crate) use shared_support::generate_axum_shared_support;

//! cratestack#1097: generated code must never rely on inference over the
//! `From` impls of `CratestackError`.
//!
//! During cratestack#1006 a public `impl From<Reject> for CratestackError`
//! landed in `cratestack-cose`; with `--features cose` generated Postgres code
//! for several schemas stopped compiling (E0282/E0283), because a `?` or
//! `.into()` in the expansion had relied on `CratestackError: From<X>`
//! having exactly one candidate.
//!
//! The orphan rule lets this test crate add its own `From<Dummy> for
//! CratestackError`, and the expansions below land in this crate, so any
//! generated site that still infers its source type over those impls fails
//! to compile here. The assertions are therefore trivial: the test is the
//! compilation.

use cratestack::CratestackError;

struct Dummy;

impl From<Dummy> for CratestackError {
    fn from(_: Dummy) -> Self {
        CratestackError::Internal("dummy".to_owned())
    }
}

mod rest_models {
    cratestack::include_server_schema!("tests/fixtures/banking_batches.cstack", db = Postgres);
}

mod rest_models_enums {
    cratestack::include_server_schema!("tests/fixtures/enum_ordinals.cstack", db = Postgres);
}

mod rpc_models {
    cratestack::include_server_schema!("tests/fixtures/enum_ordinals_rpc.cstack", db = Postgres);
}

mod rest_projection {
    cratestack::include_server_schema!("tests/fixtures/projection_tolerance.cstack", db = Postgres);
}

mod rest_computed {
    cratestack::include_server_schema!(
        "tests/fixtures/computed_fields_router.cstack",
        db = Postgres
    );
}

mod rest_idempotency {
    cratestack::include_server_schema!(
        "tests/fixtures/idempotency_runtime_rest.cstack",
        db = Postgres
    );
}

mod rpc_idempotency {
    cratestack::include_server_schema!(
        "tests/fixtures/idempotency_runtime_rpc.cstack",
        db = Postgres
    );
}

mod no_database {
    cratestack::include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);
}

mod policies {
    cratestack::include_server_schema!("tests/fixtures/advanced_policy.cstack", db = Postgres);
}

mod field_policy {
    cratestack::include_server_schema!("tests/fixtures/banking_field_policy.cstack", db = Postgres);
}

mod soft_delete_audit {
    cratestack::include_server_schema!(
        "tests/fixtures/banking_soft_delete_audit.cstack",
        db = Postgres
    );
}

mod find_many_procedure {
    cratestack::include_server_schema!("tests/fixtures/find_many_procedure.cstack", db = Postgres);
}

mod bigint_end_to_end_rpc {
    cratestack::include_server_schema!(
        "tests/fixtures/bigint_end_to_end_rpc.cstack",
        db = Postgres
    );
}

mod bytes_length_validator {
    cratestack::include_server_schema!(
        "tests/fixtures/bytes_length_validator.cstack",
        db = Postgres
    );
}

mod model_argument_validators_rest {
    cratestack::include_server_schema!(
        "tests/fixtures/model_argument_validators_rest.cstack",
        db = Postgres
    );
}

mod model_argument_validators_rpc {
    cratestack::include_server_schema!(
        "tests/fixtures/model_argument_validators_rpc.cstack",
        db = Postgres
    );
}

mod nullable_patch_clear {
    cratestack::include_server_schema!("tests/fixtures/nullable_patch_clear.cstack", db = Postgres);
}

mod keyword_fields {
    cratestack::include_server_schema!("tests/fixtures/keyword_fields.cstack", db = Postgres);
}

mod banking_validation {
    cratestack::include_server_schema!(
        "tests/fixtures/banking_validation.cstack",
        db = Postgres,
        decimal = RustDecimal
    );
}

mod cursor_list {
    cratestack::include_server_schema!("tests/fixtures/cursor_list.cstack", db = Postgres);
}

mod cursor_list_rpc {
    cratestack::include_server_schema!("tests/fixtures/cursor_list_rpc.cstack", db = Postgres);
}

mod computed_fields_rpc {
    cratestack::include_server_schema!("tests/fixtures/computed_fields_rpc.cstack", db = Postgres);
}

mod computed_fields_type_valued {
    cratestack::include_server_schema!(
        "tests/fixtures/computed_fields_type_valued.cstack",
        db = Postgres
    );
}

mod enum_query_filter {
    cratestack::include_server_schema!("tests/fixtures/enum_query_filter.cstack", db = Postgres);
}

mod enum_query_filter_rpc {
    cratestack::include_server_schema!(
        "tests/fixtures/enum_query_filter_rpc.cstack",
        db = Postgres
    );
}

mod declarative_query {
    cratestack::include_server_schema!("tests/fixtures/declarative_query.cstack", db = Postgres);
}

mod banking_versioning {
    cratestack::include_server_schema!("tests/fixtures/banking_versioning.cstack", db = Postgres);
}

mod banking_audit {
    cratestack::include_server_schema!("tests/fixtures/banking_audit.cstack", db = Postgres);
}

mod generated_defaults {
    cratestack::include_server_schema!("tests/fixtures/generated_defaults.cstack", db = Postgres);
}

mod internal_suppression_rest {
    cratestack::include_server_schema!(
        "tests/fixtures/internal_suppression_rest.cstack",
        db = Postgres
    );
}

mod internal_suppression_rpc {
    cratestack::include_server_schema!(
        "tests/fixtures/internal_suppression_rpc.cstack",
        db = Postgres
    );
}

mod builder_extensions_tier8 {
    cratestack::include_server_schema!(
        "tests/fixtures/builder_extensions_tier8.cstack",
        db = Postgres
    );
}

mod cursor_page_procedure {
    cratestack::include_server_schema!("tests/fixtures/cursor_page_procedure.cstack", db = None);
}

mod list_limit_cap {
    cratestack::include_server_schema!("tests/fixtures/list_limit_cap.cstack", db = Postgres);
}

mod optional_query_filter {
    cratestack::include_server_schema!(
        "tests/fixtures/optional_query_filter.cstack",
        db = Postgres
    );
}

mod bigint_policies {
    cratestack::include_server_schema!("tests/fixtures/bigint_policies.cstack", db = Postgres);
}

#[test]
fn generated_code_compiles_with_an_extra_cratestack_error_from_impl() {
    let err: CratestackError = Dummy.into();
    assert!(matches!(err, CratestackError::Internal(_)));
}

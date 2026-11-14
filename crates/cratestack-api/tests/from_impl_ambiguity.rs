//! cratestack#1097: generated code must never rely on inference over the
//! `From` impls of `CratestackError`. A local `From<Dummy>` makes any such
//! site ambiguous (E0282/E0283); the expansions below are the test — see
//! `cratestack-pg/tests/from_impl_ambiguity.rs` for the full story. This
//! covers the `db = None` server path (REST and RPC) and the client macro.

use cratestack::CratestackError;

struct Dummy;

impl From<Dummy> for CratestackError {
    fn from(_: Dummy) -> Self {
        CratestackError::Internal("dummy".to_owned())
    }
}

mod rest_server {
    cratestack::include_server_schema!("tests/fixtures/compact_rest.cstack", db = None);
}
mod rest_client {
    cratestack::include_client_schema!("tests/fixtures/compact_rest.cstack");
}
mod rpc_server {
    cratestack::include_server_schema!("tests/fixtures/compact_rpc.cstack", db = None);
}
mod rpc_client {
    cratestack::include_client_schema!("tests/fixtures/compact_rpc.cstack");
}
mod procedures {
    cratestack::include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);
}
mod computed {
    cratestack::include_server_schema!("tests/fixtures/computed_fields.cstack", db = None);
}

#[test]
fn generated_code_compiles_with_an_extra_cratestack_error_from_impl() {
    let err: CratestackError = Dummy.into();
    assert!(matches!(err, CratestackError::Internal(_)));
}

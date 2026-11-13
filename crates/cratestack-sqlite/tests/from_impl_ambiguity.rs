//! cratestack#1097: generated code must never rely on inference over the
//! `From` impls of `CratestackError`. A local `From<Dummy>` makes any such
//! site ambiguous (E0282/E0283); the expansions below are the test — see
//! `cratestack-pg/tests/from_impl_ambiguity.rs` for the full story. This
//! covers the embedded (rusqlite) path.

use cratestack::CratestackError;

struct Dummy;

impl From<Dummy> for CratestackError {
    fn from(_: Dummy) -> Self {
        CratestackError::Internal("dummy".to_owned())
    }
}

mod round_trip {
    cratestack::include_embedded_schema!(
        "tests/fixtures/round_trip_types.cstack",
        decimal = RustDecimal
    );
}
mod enums {
    cratestack::include_embedded_schema!("tests/fixtures/enum_ordinals.cstack");
}
mod builder {
    cratestack::include_embedded_schema!("tests/fixtures/builder_pattern.cstack");
}
mod e2e {
    cratestack::include_embedded_schema!("tests/fixtures/sqlite_e2e.cstack", decimal = RustDecimal);
}
mod views {
    cratestack::include_embedded_schema!("tests/fixtures/views_integration.cstack");
}

#[test]
fn generated_code_compiles_with_an_extra_cratestack_error_from_impl() {
    let err: CratestackError = Dummy.into();
    assert!(matches!(err, CratestackError::Internal(_)));
}

//! `@default(auth().x)` into a `BigInt` column (`CreateDefaultType::BigInt`).
//! A claim is accepted as a JSON integer or as the canonical string, the one
//! form the wire uses; every other string is refused, never coerced.

use cratestack_core::{CratestackContext, CratestackError, Value};

use super::BOUNDARIES;
use crate::query::apply_create_defaults;
use crate::{CreateDefault, CreateDefaultType, SqlValue};

fn default(nullable: bool, auth_field_required: bool) -> CreateDefault {
    CreateDefault {
        column: "tenant_id",
        auth_field: "tenantId",
        ty: CreateDefaultType::BigInt,
        nullable,
        auth_field_required,
    }
}

fn ctx(claim: Value) -> CratestackContext {
    CratestackContext::authenticated([("tenantId".to_owned(), claim)])
}

fn resolve(default: CreateDefault, ctx: &CratestackContext) -> Result<SqlValue, CratestackError> {
    apply_create_defaults(Vec::new(), &[default], ctx).map(|mut values| {
        assert_eq!(values.len(), 1);
        values.remove(0).value
    })
}

#[test]
fn an_integer_claim_becomes_a_bigint_at_every_boundary() {
    for value in BOUNDARIES {
        assert_eq!(
            resolve(default(false, true), &ctx(Value::Int(value))).unwrap(),
            SqlValue::BigInt(value)
        );
    }
}

#[test]
fn a_canonical_string_claim_becomes_a_bigint() {
    for value in BOUNDARIES {
        assert_eq!(
            resolve(default(false, true), &ctx(Value::String(value.to_string()))).unwrap(),
            SqlValue::BigInt(value)
        );
    }
}

/// Each of these parses as an `i64` through a lenient reader, which is the
/// point: the wire grammar admits none of them, so neither does a claim.
#[test]
fn a_non_canonical_or_out_of_range_string_claim_is_refused() {
    for text in [
        "+5",
        "007",
        "-0",
        " 1",
        "1 ",
        "",
        "1.0",
        "0x10",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        let err =
            resolve(default(false, true), &ctx(Value::String(text.to_owned()))).expect_err(text);
        let CratestackError::Validation(message) = err else {
            panic!("{text:?}: expected Validation, got {err:?}");
        };
        assert!(message.contains("tenantId"), "{message}");
        assert!(message.contains("tenant_id"), "{message}");
        assert!(
            !message.contains(text) || text.is_empty(),
            "echoed the claim"
        );
    }
}

#[test]
fn a_bool_or_float_claim_is_an_incompatible_type() {
    for claim in [Value::Bool(true), Value::Float(1.5)] {
        let err = resolve(default(false, true), &ctx(claim)).unwrap_err();
        assert!(matches!(err, CratestackError::Validation(_)), "{err:?}");
    }
}

/// An absent, optional claim into a nullable column is a typed NULL; into a
/// required claim or a non-nullable column it is an error, as for `Int`.
#[test]
fn an_absent_optional_claim_into_a_nullable_column_is_null_bigint() {
    let authenticated = CratestackContext::authenticated([]);
    assert_eq!(
        resolve(default(true, false), &authenticated).unwrap(),
        SqlValue::NullBigInt
    );
    assert!(matches!(
        resolve(default(false, false), &authenticated),
        Err(CratestackError::Validation(_))
    ));
    assert!(matches!(
        resolve(default(true, true), &authenticated),
        Err(CratestackError::Validation(_))
    ));
    assert!(matches!(
        resolve(default(true, false), &CratestackContext::anonymous()),
        Err(CratestackError::Forbidden(_))
    ));
}

/// The `Int` default does not learn the string form.
#[test]
fn an_int_default_still_refuses_a_string_claim() {
    let int_default = CreateDefault {
        ty: CreateDefaultType::Int,
        ..default(false, true)
    };
    assert!(matches!(
        resolve(int_default, &ctx(Value::String("7".to_owned()))),
        Err(CratestackError::Validation(_))
    ));
}

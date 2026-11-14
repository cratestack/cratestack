//! The renewal legs, `revoke`, and the shared shape checks that need no
//! enrolment context.

use cratestack_core::Schema;

use super::legs::Leg;
use super::shapes::{self, Side, Want};
use crate::diagnostics::SchemaError;

/// `beginRenewal`: exactly `deviceId` and `role` in (the interface's two
/// arguments, so a further field would have no source), a `challenge` out.
pub(super) fn begin(schema: &Schema, leg: &Leg<'_>) -> Result<(), SchemaError> {
    let input = shapes::argument(schema, leg)?;
    let output = shapes::returns(schema, leg)?;
    exactly(schema, leg, input, &["deviceId", "role"])?;
    shapes::require(schema, leg, (output, Side::Return), "challenge", Want::Text)?;
    shapes::app_fields(leg, (output, Side::Return), &["challenge"])?;
    Ok(())
}

/// `confirmRenewal`: the `signature` in, plus whatever `beginRenewal`
/// answered; the new `attestation` out.
pub(super) fn confirm(
    schema: &Schema,
    leg: &Leg<'_>,
    begin_leg: &Leg<'_>,
) -> Result<(), SchemaError> {
    let input = shapes::argument(schema, leg)?;
    let output = shapes::returns(schema, leg)?;
    let begin_output = shapes::returns(schema, begin_leg)?;
    shapes::require(
        schema,
        leg,
        (input, Side::Argument),
        "signature",
        Want::Text,
    )?;
    for field in shapes::app_fields(leg, (input, Side::Argument), &["signature"])? {
        let came_back = shapes::field(begin_output, &field.name)
            .is_some_and(|back| back.ty.name == field.ty.name && back.ty.arity == field.ty.arity);
        if !came_back {
            return Err(leg.fail(format!(
                "`{}` of its argument type `{}` has no source: it must be a field of \
                 `{}`'s return type `{}`, with the same type, so the server's own answer to \
                 leg one is sent back",
                field.name, input.name, begin_leg.procedure.name, begin_output.name
            )));
        }
    }
    shapes::require(
        schema,
        leg,
        (output, Side::Return),
        "attestation",
        Want::Text,
    )
}

/// `revoke`: exactly `deviceId` in; the answer is not read.
pub(super) fn revoke(schema: &Schema, leg: &Leg<'_>) -> Result<(), SchemaError> {
    let input = shapes::argument(schema, leg)?;
    exactly(schema, leg, input, &["deviceId"])
}

/// `ty` has the `String` fields `names` and no other.
fn exactly(
    schema: &Schema,
    leg: &Leg<'_>,
    ty: &cratestack_core::TypeDecl,
    names: &[&str],
) -> Result<(), SchemaError> {
    for name in names {
        shapes::require(schema, leg, (ty, Side::Argument), name, Want::Text)?;
    }
    if let Some(extra) = ty.fields.iter().find(|f| !names.contains(&f.name.as_str())) {
        return Err(leg.fail(format!(
            "its argument type `{}` has `{}`, which has no source: the interface passes only {}",
            ty.name,
            extra.name,
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(" and ")
        )));
    }
    Ok(())
}

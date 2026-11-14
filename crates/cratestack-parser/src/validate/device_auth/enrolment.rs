//! The two legs of an enrolment. Field names are fixed (they are
//! `DeviceAuthServer`'s own vocabulary); everything else the app sends
//! passes through `EnrolmentStart.fields`, `EnrolmentChallenge.data` and
//! `AuthUser.claims`.

use cratestack_core::schema::DeviceAuthRole;
use cratestack_core::{Schema, TypeDecl};

use super::legs::Leg;
use super::shapes::{self, Side, Want};
use crate::diagnostics::SchemaError;

/// Whether `beginEnrolment` takes a user-present key.
pub(super) fn has_user_present(begin: &TypeDecl) -> bool {
    shapes::field(begin, "userPresent").is_some()
}

/// `beginEnrolment`: the keys in (`ambient`, `userPresent?`, then the app's
/// own text fields), one challenge per key out.
pub(super) fn begin(schema: &Schema, leg: &Leg<'_>) -> Result<(), SchemaError> {
    let input = shapes::argument(schema, leg)?;
    let output = shapes::returns(schema, leg)?;
    shapes::require(schema, leg, (input, Side::Argument), "ambient", Want::Key)?;
    let user_present = has_user_present(input);
    if user_present {
        shapes::require(
            schema,
            leg,
            (input, Side::Argument),
            "userPresent",
            Want::OptionalKey,
        )?;
    }
    shapes::app_fields(leg, (input, Side::Argument), &["ambient", "userPresent"])?;
    shapes::require(schema, leg, (output, Side::Return), "ambient", Want::Text)?;
    shapes::user_present_consistent(
        schema,
        leg,
        (output, Side::Return),
        user_present,
        Want::OptionalText,
    )?;
    shapes::app_fields(leg, (output, Side::Return), &["ambient", "userPresent"])?;
    Ok(())
}

/// `confirmEnrolment`: the signed challenges in, the device, its
/// attestations and its user out.
pub(super) fn confirm(
    schema: &Schema,
    leg: &Leg<'_>,
    begin_leg: &Leg<'_>,
) -> Result<(), SchemaError> {
    let input = shapes::argument(schema, leg)?;
    let output = shapes::returns(schema, leg)?;
    let begin_input = shapes::argument(schema, begin_leg)?;
    let begin_output = shapes::returns(schema, begin_leg)?;
    let user_present = has_user_present(begin_input);

    shapes::require(schema, leg, (input, Side::Argument), "ambient", Want::Text)?;
    shapes::user_present_consistent(
        schema,
        leg,
        (input, Side::Argument),
        user_present,
        Want::OptionalText,
    )?;
    let mut known = vec!["ambient", "userPresent"];
    if let Some(proof) = &leg.proof {
        if known.contains(&proof.as_str()) {
            return Err(leg.fail(format!(
                "`proof: \"{proof}\"` is a key's signature, not the one-time code"
            )));
        }
        let Some(field) = shapes::field(input, proof) else {
            return Err(leg.fail(format!(
                "`proof: \"{proof}\"` names no field of its argument type `{}`",
                input.name
            )));
        };
        if !shapes::matches(schema, field, Want::Text)
            && !shapes::matches(schema, field, Want::OptionalText)
        {
            return Err(leg.fail(format!(
                "`proof: \"{proof}\"` is `{}{}` in its argument type `{}`: the one-time code \
                 is sent as text, so it must be `String` or `String?`",
                field.ty.name,
                match field.ty.arity {
                    cratestack_core::TypeArity::Required => "",
                    cratestack_core::TypeArity::Optional => "?",
                    cratestack_core::TypeArity::List => "[]",
                },
                input.name
            )));
        }
        known.push(proof);
    }
    for field in shapes::app_fields(leg, (input, Side::Argument), &known)? {
        let came_back = shapes::field(begin_output, &field.name)
            .is_some_and(|back| back.ty.name == field.ty.name && back.ty.arity == field.ty.arity);
        if !came_back {
            return Err(leg.fail(format!(
                "`{name}` of its argument type `{ty}` has no source: it must be a field of \
                 `{begin}`'s return type `{ret}`, with the same type, so the server's own \
                 answer to leg one is sent back (or name it with `proof:` if it is the \
                 one-time code)",
                name = field.name,
                ty = input.name,
                begin = begin_leg.procedure.name,
                ret = begin_output.name,
            )));
        }
    }

    for name in ["deviceId", "userId", "ambient"] {
        shapes::require(schema, leg, (output, Side::Return), name, Want::Text)?;
    }
    shapes::user_present_consistent(
        schema,
        leg,
        (output, Side::Return),
        user_present,
        Want::OptionalText,
    )?;
    for name in ["name", "email"] {
        if let Some(field) = shapes::field(output, name)
            && !shapes::matches(schema, field, Want::Text)
            && !shapes::matches(schema, field, Want::OptionalText)
        {
            return Err(leg.fail(format!(
                "`{name}` of its return type `{}` must be `String` or `String?`: it is the \
                 signed-in user's {name}",
                output.name
            )));
        }
    }
    shapes::app_fields(
        leg,
        (output, Side::Return),
        &[
            "deviceId",
            "userId",
            "ambient",
            "userPresent",
            "name",
            "email",
        ],
    )?;
    debug_assert_eq!(leg.role, DeviceAuthRole::ConfirmEnrolment);
    Ok(())
}

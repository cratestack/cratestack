//! Validation for the `@device_auth` procedure attribute (cratestack#1303).
//!
//! The marker makes `generate-dart --preset fespalier` emit a class that
//! implements fespalier's `DeviceAuthServer` over the generated client. That
//! class is only correct when the procedures it calls have the shapes it
//! maps, so every shape it reads is checked here, at parse time, with the
//! role and the field named: a leg whose field the generator would drop
//! (a `userPresent` that never reaches the wire) is refused, not emitted.
//!
//! Two stages. Stage 3 (`validate_procedure_device_auth_attribute`, per
//! procedure) reads the attribute with the one reader the generator uses.
//! Stage 4 (`validate_device_auth_collecting`) needs every type resolved:
//! the five interface roles are all-or-nothing, each at most once, each leg
//! public (`revoke`, which the device signs, must not be), and each leg's
//! argument and return type have the fields its role maps.

mod enrolment;
mod legs;
mod renewal;
mod shapes;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_roles;
#[cfg(test)]
mod tests_shapes;
#[cfg(test)]
mod tests_shapes_confirm;
#[cfg(test)]
mod tests_shapes_renewal;
#[cfg(test)]
mod tests_support;

use cratestack_core::schema::{
    DeviceAuthRole, device_auth_attribute, is_device_auth, parse_device_auth_attribute,
};
use cratestack_core::{Procedure, Schema};

use crate::diagnostics::{SchemaError, span_error};

/// `@device_auth` reads, appears at most once, and is on a unary call.
pub(super) fn validate_procedure_device_auth_attribute(
    procedure: &Procedure,
) -> Result<(), SchemaError> {
    let found: Vec<_> = procedure
        .attributes
        .iter()
        .filter(|attribute| is_device_auth(attribute))
        .collect();
    if let Some(repeat) = found.get(1) {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @device_auth attribute; a procedure \
                 is one leg of the ceremonies",
                procedure.name
            ),
            repeat.span,
        ));
    }
    let Some(attribute) = device_auth_attribute(&procedure.attributes) else {
        return Ok(());
    };
    parse_device_auth_attribute(&attribute.raw).map_err(|reason| {
        span_error(
            format!("procedure `{}`: {reason}", procedure.name),
            attribute.span,
        )
    })?;
    if procedure
        .attributes
        .iter()
        .any(|a| a.raw.trim() == "@stream")
    {
        return Err(span_error(
            format!(
                "procedure `{}` cannot be both `@stream` and `@device_auth`: the device \
                 ceremonies are unary calls",
                procedure.name
            ),
            attribute.span,
        ));
    }
    Ok(())
}

/// The cross-procedure rules; runs once every declaration is known good.
pub(super) fn validate_device_auth_collecting(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let found = legs::collect(schema);
    if found.is_empty() {
        return;
    }
    let before = errors.len();
    legs::roles_are_unique(&found, errors);
    legs::roles_are_complete(&found, errors);
    if errors.len() > before {
        return;
    }
    for leg in &found {
        legs::policy_matches_role(leg, errors);
    }
    let by_role = |role: DeviceAuthRole| found.iter().find(|leg| leg.role == role);
    for leg in &found {
        let checked = match leg.role {
            DeviceAuthRole::BeginEnrolment => enrolment::begin(schema, leg),
            DeviceAuthRole::ConfirmEnrolment => {
                let begin = by_role(DeviceAuthRole::BeginEnrolment).expect("roles are complete");
                enrolment::confirm(schema, leg, begin)
            }
            DeviceAuthRole::BeginRenewal => renewal::begin(schema, leg),
            DeviceAuthRole::ConfirmRenewal => {
                let begin = by_role(DeviceAuthRole::BeginRenewal).expect("roles are complete");
                renewal::confirm(schema, leg, begin)
            }
            DeviceAuthRole::Revoke => renewal::revoke(schema, leg),
            // Passed through whole: the generated method takes the procedure's own
            // argument wrapper and returns its answer.
            DeviceAuthRole::SendEnrolmentCode => Ok(()),
        };
        if let Err(error) = checked {
            errors.push(error);
        }
    }
}

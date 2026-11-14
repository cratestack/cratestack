//! The views of the two enrolment legs, and the JWK helpers they share.
//! Field names are `DeviceAuthServer`'s own vocabulary (`ambient`,
//! `userPresent`, `deviceId`, `userId`, `name`, `email`); every other field
//! passes through `EnrolmentStart.fields`, `EnrolmentChallenge.data` and
//! `AuthUser.claims`.
use cratestack_core::TypeDecl;
use cratestack_core::schema::DeviceAuthRole;

use super::legs::{Legs, answer, answer_element, entry, field, refuse, text_from, text_of};
use super::view::{BeginEnrolmentView, ConfirmEnrolmentView, Entry, JwkHelper, SendCodeView};
use crate::config::DartGeneratorError;
use crate::dart_types::dart_type;
use crate::idents::{dart_identifier, to_camel_case};
use crate::naming::{occupied_type_names, procedure_wrapper_name};

const KEYS: [&str; 2] = ["ambient", "userPresent"];

pub(super) fn send_code(legs: &Legs<'_>) -> Result<Option<SendCodeView>, DartGeneratorError> {
    let Some((procedure, _)) = legs.get(DeviceAuthRole::SendEnrolmentCode) else {
        return Ok(None);
    };
    // The wrapper is passed whole: the method takes the generated arguments
    // class and answers what the procedure answers.
    Ok(Some(SendCodeView {
        method: to_camel_case(&procedure.name),
        wrapper: procedure_wrapper_name(procedure, &occupied_type_names(legs.schema)),
        return_type: dart_type(&procedure.return_type, false),
    }))
}

pub(super) fn begin(
    legs: &Legs<'_>,
    jwks: &mut Vec<JwkHelper>,
) -> Result<BeginEnrolmentView, DartGeneratorError> {
    let (procedure, _) = legs.required(DeviceAuthRole::BeginEnrolment);
    let (input, output) = (legs.argument(procedure)?, legs.returns(procedure)?);
    // One helper, `_jwk`, when both keys are the same `type` (the usual case).
    let key_types: std::collections::BTreeSet<&str> = KEYS
        .iter()
        .filter_map(|name| field(input, name))
        .map(|f| f.ty.name.as_str())
        .collect();
    let single = key_types.len() == 1;
    let mut args = Vec::new();
    for f in &input.fields {
        let expr = match f.name.as_str() {
            "ambient" => format!(
                "{}(start.keys[KeyRole.ambient]!)",
                jwk_helper(legs, &f.ty, single, jwks)?
            ),
            "userPresent" => format!(
                "switch (start.keys[KeyRole.userPresent]) {{ final jwk? => {}(jwk), null => null }}",
                jwk_helper(legs, &f.ty, single, jwks)?
            ),
            _ => text_from("start.fields", f),
        };
        args.push(entry(f, expr));
    }
    let mut challenges = vec!["KeyRole.ambient: answer.ambient".to_owned()];
    if field(output, "userPresent").is_some() {
        challenges.push("KeyRole.userPresent: ?answer.userPresent".to_owned());
    }
    let data = output
        .fields
        .iter()
        .filter(|f| !KEYS.contains(&f.name.as_str()))
        .map(answer_element)
        .collect();
    Ok(BeginEnrolmentView {
        leg: legs.leg(procedure, args)?,
        challenges,
        data,
    })
}

pub(super) fn confirm(legs: &Legs<'_>) -> Result<ConfirmEnrolmentView, DartGeneratorError> {
    let (procedure, parsed) = legs.required(DeviceAuthRole::ConfirmEnrolment);
    let (input, output) = (legs.argument(procedure)?, legs.returns(procedure)?);
    let mut args = Vec::new();
    for f in &input.fields {
        let expr = match f.name.as_str() {
            "ambient" => "challengeSignatures[KeyRole.ambient]!".to_owned(),
            "userPresent" => "challengeSignatures[KeyRole.userPresent]".to_owned(),
            name if parsed.proof.as_deref() == Some(name) => text_of("proof", f),
            _ => text_from("challenge.data", f),
        };
        args.push(entry(f, expr));
    }
    let mut attestations = vec!["KeyRole.ambient: Attestation.parse(answer.ambient)".to_owned()];
    if field(output, "userPresent").is_some() {
        attestations.push(
            "if (answer.userPresent case final jws?) KeyRole.userPresent: Attestation.parse(jws)"
                .to_owned(),
        );
    }
    let present = |name: &str| field(output, name).map(answer);
    let claims = output
        .fields
        .iter()
        .filter(|f| !is_known(&f.name))
        .map(answer_element)
        .collect();
    Ok(ConfirmEnrolmentView {
        leg: legs.leg(procedure, args)?,
        answer_type: output.name.clone(),
        attestations,
        proof: parsed.proof.clone(),
        name_expr: present("name"),
        email_expr: present("email"),
        claims,
    })
}

/// The answer's fields `DeviceEnrolment` and `AuthUser` have a place for.
fn is_known(name: &str) -> bool {
    [
        "deviceId",
        "userId",
        "ambient",
        "userPresent",
        "name",
        "email",
    ]
    .contains(&name)
}

/// The helper that builds the schema JWK `type` of `ty` from the
/// `Map<String, String>` a `EnrolmentStart.keys` entry is.
fn jwk_helper(
    legs: &Legs<'_>,
    ty: &cratestack_core::TypeRef,
    single: bool,
    jwks: &mut Vec<JwkHelper>,
) -> Result<String, DartGeneratorError> {
    let decl: &TypeDecl = legs
        .schema
        .types
        .iter()
        .find(|decl| decl.name == ty.name)
        .ok_or_else(|| refuse(format!("`{}` is not a JWK `type`", ty.name)))?;
    if let Some(known) = jwks.iter().find(|jwk| jwk.type_name == decl.name) {
        return Ok(known.name.clone());
    }
    let fields = decl
        .fields
        .iter()
        .map(|f| Entry {
            name: dart_identifier(&f.name),
            expr: format!("jwk['{}']!", f.name),
        })
        .collect();
    let name = if single {
        "_jwk".to_owned()
    } else {
        format!("_jwkOf{}", decl.name)
    };
    jwks.push(JwkHelper {
        name: name.clone(),
        type_name: decl.name.clone(),
        fields,
    });
    Ok(name)
}

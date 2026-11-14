//! The marked procedures, and the rules that need only their roles and
//! policies: each role once, the five interface roles together, public legs
//! public, `revoke` not.

use cratestack_core::schema::{DeviceAuthRole, device_auth_attribute, procedure_device_auth};
use cratestack_core::{Procedure, Schema, SourceSpan};

use crate::diagnostics::{SchemaError, span_error};

/// One `@device_auth` procedure.
pub(super) struct Leg<'a> {
    pub(super) role: DeviceAuthRole,
    pub(super) proof: Option<String>,
    pub(super) procedure: &'a Procedure,
    /// The marker's own span: where every error about this leg points.
    pub(super) span: SourceSpan,
}

impl Leg<'_> {
    /// An error at the marker, naming the procedure and the role.
    pub(super) fn fail(&self, why: impl std::fmt::Display) -> SchemaError {
        span_error(
            format!(
                "procedure `{}` (`@device_auth(role: \"{}\")`): {why}",
                self.procedure.name,
                self.role.name()
            ),
            self.span,
        )
    }
}

/// Every unary procedure with a readable `@device_auth`, in source order.
pub(super) fn collect(schema: &Schema) -> Vec<Leg<'_>> {
    schema
        .procedures
        .iter()
        .filter_map(|procedure| {
            let parsed = procedure_device_auth(procedure)?;
            let attribute = device_auth_attribute(&procedure.attributes)?;
            Some(Leg {
                role: parsed.role,
                proof: parsed.proof,
                procedure,
                span: attribute.span,
            })
        })
        .collect()
}

pub(super) fn roles_are_unique(legs: &[Leg<'_>], errors: &mut Vec<SchemaError>) {
    for (at, leg) in legs.iter().enumerate() {
        if let Some(first) = legs[..at].iter().find(|earlier| earlier.role == leg.role) {
            errors.push(
                leg.fail(format!(
                    "`role: \"{}\"` is already on procedure `{}` (line {}); each role is one \
                     procedure",
                    leg.role.name(),
                    first.procedure.name,
                    first.span.line
                ))
                .with_related("the first one", first.span),
            );
        }
    }
}

pub(super) fn roles_are_complete(legs: &[Leg<'_>], errors: &mut Vec<SchemaError>) {
    let missing: Vec<String> = DeviceAuthRole::REQUIRED
        .into_iter()
        .filter(|role| !legs.iter().any(|leg| leg.role == *role))
        .map(|role| format!("`{}`", role.name()))
        .collect();
    if missing.is_empty() {
        return;
    }
    let present: Vec<String> = legs
        .iter()
        .map(|leg| format!("`{}`", leg.role.name()))
        .collect();
    errors.push(span_error(
        format!(
            "`@device_auth` marks {} but not {}: the device roles are all-or-nothing, because \
             the generated `DeviceAuthServer` cannot be implemented without every one of {}",
            present.join(", "),
            missing.join(", "),
            DeviceAuthRole::REQUIRED
                .map(|role| format!("`{}`", role.name()))
                .join(", ")
        ),
        legs[0].span,
    ));
}

/// The policy a leg needs: public (exactly `@allow(true)`) for every leg a
/// session-less device calls, and anything that does not admit a caller with
/// no session for `revoke` (decided on the policy's meaning, see
/// [`super::anonymous`]).
pub(super) fn policy_matches_role(leg: &Leg<'_>, errors: &mut Vec<SchemaError>) {
    let policy: Vec<&str> = leg
        .procedure
        .attributes
        .iter()
        .map(|attribute| attribute.raw.trim())
        .filter(|raw| {
            ["@allow", "@deny", "@authn", "@authorize"]
                .iter()
                .any(|name| raw.starts_with(name))
        })
        .collect();
    let public = |raw: &&str| {
        raw.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            == "@allow(true)"
    };
    if leg.role.is_public() {
        if !(policy.len() == 1 && public(&policy[0])) {
            let found = if policy.is_empty() {
                "it has no policy".to_owned()
            } else {
                format!("it writes {}", policy.join(" "))
            };
            errors.push(leg.fail(format!(
                "a device has no session when it calls this, so the procedure must be public: \
                 exactly `@allow(true)`, with no `@deny`, `@authn` or `@authorize` ({found})"
            )));
        }
    } else if super::anonymous::admits_anonymous(leg.procedure) {
        errors.push(leg.fail(
            "`revoke` must not be public: anyone could revoke any device by its id, and its \
             policy admits a caller with no session. The device signs this call, so give it \
             a policy that needs the signed session, such as `@allow(auth() != null)` (a \
             procedure with no `@allow` is denied to everyone, which is not public but means \
             no device can be revoked either)",
        ));
    }
}

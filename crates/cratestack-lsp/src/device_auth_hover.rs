//! Hover for `@device_auth(role: "...")` on a procedure (cratestack#1303):
//! the role and the `DeviceAuthServer` method it becomes.

use cratestack_core::Schema;
use cratestack_core::schema::{DeviceAuthRole, device_auth_attribute, parse_device_auth_attribute};

use crate::state::SymbolInfo;
use crate::text::span_contains;

pub(crate) fn hover_symbol(schema: &Schema, offset: usize) -> Option<SymbolInfo> {
    schema.procedures.iter().find_map(|procedure| {
        let attribute = device_auth_attribute(&procedure.attributes)?;
        if !span_contains(attribute.span, offset) {
            return None;
        }
        let parsed = parse_device_auth_attribute(&attribute.raw).ok()?;
        Some(SymbolInfo {
            kind: "device auth",
            name: parsed.role.name().to_owned(),
            detail: format!("procedure `{}`: {}", procedure.name, describe(parsed.role)),
            docs: Vec::new(),
            selection_span: attribute.span,
        })
    })
}

fn describe(role: DeviceAuthRole) -> &'static str {
    match role {
        DeviceAuthRole::SendEnrolmentCode => {
            "asks the server to send the one-time code, before leg one (an extra method of \
             the generated class, not part of `DeviceAuthServer`)"
        }
        DeviceAuthRole::BeginEnrolment => {
            "`DeviceAuthServer.beginEnrolment`: the device's public keys in, one challenge per key out"
        }
        DeviceAuthRole::ConfirmEnrolment => {
            "`DeviceAuthServer.confirmEnrolment`: the signed challenges in, the device, its \
             attestations and its user out"
        }
        DeviceAuthRole::BeginRenewal => {
            "`DeviceAuthServer.beginRenewal`: a device and a key role in, a challenge out; a 403 \
             is `AuthRejected`"
        }
        DeviceAuthRole::ConfirmRenewal => {
            "`DeviceAuthServer.confirmRenewal`: the signature in, a new attestation out; a 403 \
             is `AuthRejected`"
        }
        DeviceAuthRole::Revoke => {
            "`DeviceAuthServer.revoke`: tells the server the device is gone, signed by the \
             device at sign-out; not public"
        }
    }
}

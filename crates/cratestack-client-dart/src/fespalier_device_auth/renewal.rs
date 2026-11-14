//! The views of the renewal legs and of `revoke`.
use cratestack_core::schema::DeviceAuthRole;

use super::legs::{Legs, answer_element, entry, field, refuse, text_from};
use super::view::{BeginRenewalView, Entry, LegView};
use crate::config::DartGeneratorError;

/// `beginRenewal`: `deviceId` and `role` in, a challenge and its data out.
pub(super) fn begin(legs: &Legs<'_>) -> Result<BeginRenewalView, DartGeneratorError> {
    let (procedure, _) = legs.required(DeviceAuthRole::BeginRenewal);
    let (input, output) = (legs.argument(procedure)?, legs.returns(procedure)?);
    let args = input
        .fields
        .iter()
        .map(|f| {
            let expr = if f.name == "role" {
                "role.name"
            } else {
                "deviceId"
            };
            entry(f, expr.to_owned())
        })
        .collect();
    let data = output
        .fields
        .iter()
        .filter(|f| f.name != "challenge")
        .map(answer_element)
        .collect();
    Ok(BeginRenewalView {
        leg: legs.leg(procedure, args)?,
        data,
    })
}

/// `confirmRenewal`: the `signature` and what leg one answered.
pub(super) fn confirm(legs: &Legs<'_>) -> Result<LegView, DartGeneratorError> {
    let (procedure, _) = legs.required(DeviceAuthRole::ConfirmRenewal);
    let input = legs.argument(procedure)?;
    let args = input
        .fields
        .iter()
        .map(|f| {
            let expr = if f.name == "signature" {
                "signature".to_owned()
            } else {
                text_from("challenge.data", f)
            };
            entry(f, expr)
        })
        .collect();
    legs.leg(procedure, args)
}

/// `revoke`: the credential's device id.
pub(super) fn revoke(legs: &Legs<'_>) -> Result<LegView, DartGeneratorError> {
    let (procedure, _) = legs.required(DeviceAuthRole::Revoke);
    let input = legs.argument(procedure)?;
    let device_id = field(input, "deviceId")
        .ok_or_else(|| refuse("the `revoke` argument has no `deviceId`"))?;
    let args: Vec<Entry> = vec![entry(device_id, "credential.deviceId".to_owned())];
    legs.leg(procedure, args)
}

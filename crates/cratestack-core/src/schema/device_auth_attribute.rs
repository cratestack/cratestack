//! The procedure attribute `@device_auth(role: "beginEnrolment")`
//! (cratestack#1303): marks one procedure as a leg of a device's enrolment,
//! renewal or revocation, so `generate-dart --preset fespalier` can emit a
//! `DeviceAuthServer` over the generated client.
//!
//! One reader for every consumer: `cratestack-parser` refuses a marker it
//! does not read, and the Dart generator turns what it reads into the
//! class. The attribute stays a raw [`Attribute`] in `Procedure::attributes`,
//! as `@authn` does. Keys are `role` (required, one of
//! [`DeviceAuthRole::ALL`]) and, on `confirmEnrolment` only, `proof` (the
//! argument field that carries the one-time code); each at most once.

use super::{Attribute, DeviceAuthRole, Procedure};

/// The attribute's name.
pub const DEVICE_AUTH_ATTRIBUTE: &str = "@device_auth";

/// The keys `@device_auth` takes, in the order its error messages list them.
pub const DEVICE_AUTH_KEYS: [&str; 2] = ["role", "proof"];

/// What a `@device_auth` attribute says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDeviceAuth {
    /// `role`.
    pub role: DeviceAuthRole,
    /// `proof`: the field of the `confirmEnrolment` argument that carries
    /// the one-time code. Without it the proof is ignored.
    pub proof: Option<String>,
}

/// The `@device_auth` among `attributes`, if any (the first, when a schema
/// repeats it; the parser refuses a repeat).
pub fn device_auth_attribute(attributes: &[Attribute]) -> Option<&Attribute> {
    attributes
        .iter()
        .find(|attribute| is_device_auth(attribute))
}

/// Whether `attribute` is a `@device_auth`, whatever its arguments.
pub fn is_device_auth(attribute: &Attribute) -> bool {
    attribute
        .raw
        .trim()
        .strip_prefix(DEVICE_AUTH_ATTRIBUTE)
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// What `procedure`'s `@device_auth` says, when it has one the reader accepts.
pub fn procedure_device_auth(procedure: &Procedure) -> Option<ParsedDeviceAuth> {
    let attribute = device_auth_attribute(&procedure.attributes)?;
    parse_device_auth_attribute(&attribute.raw).ok()
}

/// Reads a whole `@device_auth(...)` attribute.
pub fn parse_device_auth_attribute(raw: &str) -> Result<ParsedDeviceAuth, String> {
    let roles = || {
        DeviceAuthRole::ALL
            .map(|role| format!("`{}`", role.name()))
            .join(", ")
    };
    let inner = raw
        .trim()
        .strip_prefix(DEVICE_AUTH_ATTRIBUTE)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| {
            format!(
                "`@device_auth` takes arguments in parentheses, e.g. \
                 `@device_auth(role: \"beginEnrolment\")`; the roles are {}",
                roles()
            )
        })?;
    let (mut role, mut proof, mut seen) = (None, None, Vec::new());
    for argument in split_arguments(inner)? {
        let (key, value) = argument
            .split_once(':')
            .map(|(key, value)| (key.trim(), value.trim()))
            .ok_or_else(|| {
                format!("`@device_auth` argument `{argument}` is not `key: \"value\"`")
            })?;
        if !DEVICE_AUTH_KEYS.contains(&key) {
            return Err(format!(
                "`@device_auth` has no argument `{key}`; it takes {}",
                DEVICE_AUTH_KEYS.map(|key| format!("`{key}`")).join(", ")
            ));
        }
        if seen.contains(&key) {
            return Err(format!("`@device_auth` repeats `{key}`"));
        }
        seen.push(key);
        let text = ['"', '\'']
            .into_iter()
            .find_map(|quote| value.strip_prefix(quote)?.strip_suffix(quote))
            .ok_or_else(|| {
                format!("`@device_auth` `{key}` must be a quoted string, not `{value}`")
            })?;
        if key == "role" {
            role = Some(DeviceAuthRole::from_name(text).ok_or_else(|| {
                format!(
                    "`@device_auth` has no role `{text}`; the roles are {}",
                    roles()
                )
            })?);
        } else {
            proof = Some(identifier(text)?);
        }
    }
    let role = role.ok_or_else(|| format!("`@device_auth` needs `role`, one of {}", roles()))?;
    if proof.is_some() && role != DeviceAuthRole::ConfirmEnrolment {
        return Err(format!(
            "`@device_auth` takes `proof` only on `role: \"confirmEnrolment\"`, not `{}`",
            role.name()
        ));
    }
    Ok(ParsedDeviceAuth { role, proof })
}

fn identifier(text: &str) -> Result<String, String> {
    let mut chars = text.chars();
    let plain = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        Ok(text.to_owned())
    } else {
        Err(format!(
            "`@device_auth` `proof` must name a field of the argument type, not `{text}`"
        ))
    }
}

/// `text` split at commas outside quotes, each part trimmed; an empty text
/// is no parts, an empty part between commas is an error.
fn split_arguments(text: &str) -> Result<Vec<&str>, String> {
    let (mut parts, mut quote, mut start) = (Vec::new(), None::<char>, 0);
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (Some(open), _) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, ',') => {
                parts.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return Err("`@device_auth` has an unclosed quote".to_owned());
    }
    let last = text[start..].trim();
    if !last.is_empty() || !parts.is_empty() {
        parts.push(last);
    }
    if parts.iter().any(|part| part.is_empty()) {
        return Err("`@device_auth` has an empty argument".to_owned());
    }
    Ok(parts)
}

#[cfg(test)]
#[path = "device_auth_attribute_tests.rs"]
mod tests;

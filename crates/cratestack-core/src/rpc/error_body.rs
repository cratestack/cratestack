//! [`RpcErrorBody`], the wire shape of one RPC error, with the extra
//! members an [`AuthRefusal`](crate::AuthRefusal) adds (cratestack#1208).
//!
//! Its serde is written by hand for the reason given in
//! `error/response.rs`: a known-length map, so a body without extra
//! members encodes exactly as it did before.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{cratestack_error_code_to_rpc_code, rpc_code};
use crate::error::{CratestackError, CratestackErrorResponse, visible_extra_members};

/// Wire shape of a single error returned by an RPC call. Maps from
/// [`CratestackError`] via [`rpc_code`] + [`CratestackError::public_message`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RpcErrorBody {
    /// Stable gRPC-style code: `not_found`, `invalid_argument`,
    /// `permission_denied`, `failed_precondition`, `conflict`,
    /// `unauthenticated`, `resource_exhausted` (a rate-limit throttle,
    /// cratestack#846), `aborted` (an `@isolation` transaction that ran out
    /// of retries; safe to send again), `unavailable`, `internal`.
    pub code: String,
    /// Public, safe-to-expose message.
    pub message: String,
    /// Op-defined structured payload (e.g. validation issues). Omitted on
    /// the wire when `None`.
    pub details: Option<serde_json::Value>,
    /// Extra top-level members after `code`, `message` and `details`, set
    /// by an [`AuthRefusal`](crate::AuthRefusal). Reserved names are never
    /// written; a body read from the wire collects every member it does not
    /// know here.
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl RpcErrorBody {
    pub fn from_cratestack(error: &CratestackError) -> Self {
        let code = rpc_code(error).to_owned();
        let CratestackErrorResponse {
            message,
            details,
            extra,
            ..
        } = error.clone_envelope();
        Self {
            code,
            message,
            details: details.map(cratestack_value_to_json),
            extra: extra
                .into_iter()
                .map(|(name, value)| (name, cratestack_value_to_json(value)))
                .collect(),
        }
    }

    /// Translate a REST-style [`CratestackErrorResponse`] into the RPC
    /// error body. The `code` field is mapped from screaming-snake to
    /// gRPC-style lowercase via [`cratestack_error_code_to_rpc_code`];
    /// `message`, `details` and the extra members flow through verbatim.
    pub fn from_cratestack_response(response: CratestackErrorResponse) -> Self {
        let CratestackErrorResponse {
            code,
            message,
            details,
            extra,
        } = response;
        Self {
            code: cratestack_error_code_to_rpc_code(&code).to_owned(),
            message,
            details: details.map(cratestack_value_to_json),
            extra: extra
                .into_iter()
                .map(|(name, value)| (name, cratestack_value_to_json(value)))
                .collect(),
        }
    }
}

fn cratestack_value_to_json(value: crate::Value) -> serde_json::Value {
    serde_json::to_value(&value).unwrap_or(serde_json::Value::Null)
}

impl Serialize for RpcErrorBody {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len =
            2 + usize::from(self.details.is_some()) + visible_extra_members(&self.extra).count();
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("code", &self.code)?;
        map.serialize_entry("message", &self.message)?;
        if let Some(details) = &self.details {
            map.serialize_entry("details", details)?;
        }
        for (name, value) in visible_extra_members(&self.extra) {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for RpcErrorBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(BodyVisitor)
    }
}

struct BodyVisitor;

impl<'de> Visitor<'de> for BodyVisitor {
    type Value = RpcErrorBody;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an RPC error body")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let (mut code, mut message, mut details) = (None, None, None);
        let mut extra = BTreeMap::new();
        while let Some(name) = map.next_key::<String>()? {
            match name.as_str() {
                "code" => code = Some(map.next_value()?),
                "message" => message = Some(map.next_value()?),
                "details" => details = map.next_value()?,
                _ => {
                    let value = map.next_value()?;
                    extra.insert(name, value);
                }
            }
        }
        Ok(RpcErrorBody {
            code: code.ok_or_else(|| serde::de::Error::missing_field("code"))?,
            message: message.ok_or_else(|| serde::de::Error::missing_field("message"))?,
            details,
            extra,
        })
    }
}

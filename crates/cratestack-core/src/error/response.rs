//! [`CratestackErrorResponse`], the error envelope a REST client receives,
//! and its serde: a map of `code`, `message`, `details`, then any extra
//! members an [`AuthRefusal`](super::AuthRefusal) added (cratestack#1208).
//!
//! The serde is written by hand rather than with `#[serde(flatten)]`:
//! `flatten` serializes a map of unknown length, which a CBOR encoder
//! writes as an indefinite-length map, and every envelope on the wire
//! would change shape. Here the length is always known, and an envelope
//! without extra members encodes byte for byte as it did before.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::CratestackError;
use super::auth_refusal::RESERVED_ENVELOPE_MEMBERS;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CratestackErrorResponse {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
    /// Extra top-level members, after `code`, `message` and `details`.
    /// Only an [`AuthRefusal`](super::AuthRefusal) sets them today. A name
    /// in [`RESERVED_ENVELOPE_MEMBERS`] is never written, and an envelope
    /// read from the wire collects here every member it does not know.
    pub extra: BTreeMap<String, Value>,
}

pub(crate) fn visible_extra_members<V>(
    extra: &BTreeMap<String, V>,
) -> impl Iterator<Item = (&String, &V)> {
    extra
        .iter()
        .filter(|(name, _)| !RESERVED_ENVELOPE_MEMBERS.contains(&name.as_str()))
}

impl Serialize for CratestackErrorResponse {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = 3 + visible_extra_members(&self.extra).count();
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("code", &self.code)?;
        map.serialize_entry("message", &self.message)?;
        map.serialize_entry("details", &self.details)?;
        for (name, value) in visible_extra_members(&self.extra) {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CratestackErrorResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(EnvelopeVisitor)
    }
}

struct EnvelopeVisitor;

impl<'de> Visitor<'de> for EnvelopeVisitor {
    type Value = CratestackErrorResponse;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a CrateStack error envelope")
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
        Ok(CratestackErrorResponse {
            code: code.ok_or_else(|| serde::de::Error::missing_field("code"))?,
            message: message.ok_or_else(|| serde::de::Error::missing_field("message"))?,
            details,
            extra,
        })
    }
}

// The envelope a `CratestackError` answers with: here, next to the
// envelope's own serde, rather than in `error.rs`.
impl CratestackError {
    /// The `WWW-Authenticate` header values the response carries: an
    /// [`AuthRefusal`]'s challenges, and none for every other error.
    pub fn auth_challenges(&self) -> &[String] {
        match self {
            Self::AuthRefused(refusal) => refusal.challenges(),
            _ => &[],
        }
    }

    pub fn into_response(self) -> CratestackErrorResponse {
        let code = self.code().to_owned();
        match self {
            Self::AuthRefused(refusal) => {
                let (message, details, extra) = refusal.into_parts();
                CratestackErrorResponse {
                    code,
                    message,
                    details,
                    extra,
                }
            }
            other => CratestackErrorResponse {
                code,
                message: other.public_message().into_owned(),
                ..Default::default()
            },
        }
    }

    /// The envelope [`Self::into_response`] returns, without consuming
    /// the error: `RpcErrorBody::from_cratestack` builds the RPC body from a
    /// borrowed error, so a refusal's members are copied here, and only
    /// on that path.
    pub(crate) fn clone_envelope(&self) -> CratestackErrorResponse {
        let code = self.code().to_owned();
        let message = self.public_message().into_owned();
        match self {
            Self::AuthRefused(refusal) => CratestackErrorResponse {
                code,
                message,
                details: refusal.details().cloned(),
                extra: refusal.members().clone(),
            },
            _ => CratestackErrorResponse {
                code,
                message,
                ..Default::default()
            },
        }
    }
}

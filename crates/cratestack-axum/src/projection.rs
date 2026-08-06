//! Format-preserving intermediate representation for generated
//! `list`/`get` model-response projection (cratestack#430).
//!
//! Before this type existed, `project_<model>_model_value`
//! (`cratestack-macros`' `axum/model/serializers.rs`) routed every row
//! through `serde_json::to_value`. `serde_json::Value` always reports
//! itself human-readable (`Serializer::is_human_readable` — see
//! `serde_json::value::Serializer`), so any field whose own `Serialize`
//! impl branches on that hint (`uuid::Uuid`, `chrono::DateTime`, …) took
//! its *string* branch right there, permanently — before the real wire
//! codec ever ran. Re-encoding that already-a-string value through CBOR
//! (`minicbor-serde`, which correctly reports itself non-human-readable)
//! produced a CBOR text string; the generated client decodes straight
//! into `uuid::Uuid`, whose `Deserialize` takes the *bytes* branch under
//! a non-human-readable format — a decode error on every `Uuid` column,
//! every time, over the default (CBOR) wire format.
//!
//! `ProjectedValue` defers that branch to the real target `Serializer`
//! instead of baking it in early: each scalar leaf keeps the record
//! field's *original* value behind a type-erased `erased_serde::Serialize`
//! trait object (`erased-serde` exists specifically to make `Serialize`
//! object-safe; see its crate docs) rather than a pre-serialized
//! `serde_json::Value`. When the response is finally encoded — by
//! `JsonCodec` or `CborCodec`, chosen per-request via content
//! negotiation, long after projection ran — `erased_serde::serialize`
//! drives the leaf's *original* `Serialize::serialize` against the real
//! serializer, so `is_human_readable()` reports the truth and the right
//! branch runs. `Null` gets its own variant that always calls
//! `serialize_none()` (the same primitive `Option::<T>::None` uses),
//! rather than piggybacking on `serde_json::Value::Null`'s
//! `serialize_unit()` — which is what the old code additionally had to
//! special-case (a documented, separate `minicbor-serde` quirk: unit
//! encodes as a CBOR empty array, not null). That workaround — stripping
//! `Null` map entries out of the top-level projection before the codec
//! ever saw them — is gone: it's no longer needed for scalar columns,
//! and it was never applied to nullable to-one relation includes in the
//! first place (a latent, separate CBOR-null bug on that path, fixed as
//! a natural side effect of routing both through the same correct
//! `Null` variant).

use std::collections::BTreeMap;

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

/// One projected model field, or a nested included relation. See the
/// module doc for why this replaces `serde_json::Value` on the
/// projection path.
pub enum ProjectedValue {
    /// Absent value — a `None` scalar, or a missing nullable to-one
    /// relation. Always serializes via `serialize_none()`, matching
    /// `Option::<T>::None`'s own wire encoding under every codec.
    Null,
    /// A single scalar field, holding the record field's *original*
    /// value so its own `Serialize` impl runs against the real target
    /// serializer at encode time. Construct via [`ProjectedValue::leaf`].
    Leaf(Box<dyn erased_serde::Serialize + Send + Sync>),
    /// A projected model (the detail shape, or one list element).
    /// `BTreeMap` mirrors `serde_json::Map`'s own default (non
    /// `preserve_order`) key ordering, which the rest of the generated
    /// projection code already relies on being alphabetical.
    Object(BTreeMap<String, ProjectedValue>),
    /// A to-many included relation.
    Array(Vec<ProjectedValue>),
}

impl ProjectedValue {
    /// Wrap a single field's value without collapsing its type-specific
    /// `Serialize` behavior. `T` is kept — not pre-serialized — so
    /// `is_human_readable()`-sensitive impls (`Uuid`, `chrono::DateTime`,
    /// `Option<T>`, …) see the *real* target serializer later.
    pub fn leaf<T>(value: T) -> Self
    where
        T: Serialize + Send + Sync + 'static,
    {
        ProjectedValue::Leaf(Box::new(value))
    }
}

impl Serialize for ProjectedValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ProjectedValue::Null => serializer.serialize_none(),
            // Bridges the erased trait object back into the *concrete*
            // target serializer — this, not any special-casing here, is
            // what makes `is_human_readable()` report the real wire
            // format to the leaf's own `Serialize` impl.
            ProjectedValue::Leaf(value) => erased_serde::serialize(value.as_ref(), serializer),
            ProjectedValue::Object(object) => {
                let mut map = serializer.serialize_map(Some(object.len()))?;
                for (key, value) in object {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            ProjectedValue::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
        }
    }
}

#[cfg(test)]
mod tests;

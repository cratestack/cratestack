//! Shared "double `Option`" (de)serialization for `Update{Model}Input`
//! fields that back a nullable column — see cratestack#567.
//!
//! `Update{Model}Input` wraps every field in an outer `Option<T>` ("was
//! this field touched by this patch at all") on top of whatever the
//! column's own nullability already contributes. For a nullable column
//! that's a *second*, independent `Option` layer: `None` (field omitted),
//! `Some(None)` (field explicitly set to SQL `NULL`), `Some(Some(v))`
//! (field set to `v`). serde-derive's blanket `Option<T>: Deserialize`
//! only ever peels one layer — a JSON/CBOR `null` and an absent key both
//! collapse to the outer `None` via `visit_none()`, so `Some(None)` was
//! never reachable over the wire and "clear this column" silently did
//! nothing (`update_sql_value` skips a `None` field).
//!
//! [`deserialize_double_option`] is the fix for the inbound side: paired
//! with `#[serde(default, ...)]` (required because a custom
//! `deserialize_with` opts a field out of serde-derive's own implicit
//! "missing `Option<T>` field defaults to `None`" handling), it recurses
//! into the inner `Option` instead of stopping at the outer one.
//!
//! The outbound side needs an equally deliberate fix, not just the
//! inverse of this function: `Update{Model}Input`'s derived `Serialize`
//! has no custom logic at all, so an untouched field (outer `None`) is
//! *not* omitted from the wire by default — it serializes as `null`,
//! identical to an explicit clear. Every generated caller (the Rust/Dart/
//! TypeScript HTTP clients) builds a full `Update{Model}Input` with
//! `..Default::default()` for untouched fields and serializes the whole
//! struct, so without a fix here *every* partial update over those
//! clients would start nulling out every field it didn't set the moment
//! [`deserialize_double_option`] made the server actually honour `null`
//! as "clear". The generated field attribute pairs this function with
//! `#[serde(skip_serializing_if = "Option::is_none")]` on the *outer*
//! `Option`, so an untouched field is omitted from the wire entirely
//! (never sent as `null`) and only `Some(_)` — a field this patch
//! genuinely touches — is serialized at all.

/// Deserializes `Option<Option<T>>` distinguishing "key absent" from
/// "key present with value `null`". Pair with `#[serde(default, ...)]` —
/// see the module doc for why `default` is required once a field opts
/// into a custom `deserialize_with`.
pub fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    // Delegates to `Option<T>`'s own (blanket) `Deserialize` impl to get
    // one full "was this JSON `null`" answer, then wraps it in `Some` to
    // record "yes, this key was present" at the outer layer. `deserialize`
    // is only ever called when the field's key IS present in the map —
    // serde-derive's generated visitor only invokes a field's
    // `deserialize_with` from its `Some(key) => ...` arm; an absent key
    // instead falls through to `#[serde(default)]`, which for `Option<T>`
    // is `Option::default()` i.e. plain `None`. So this function itself
    // only ever needs to produce `Some(...)`.
    <Option<T> as serde::Deserialize<'de>>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::deserialize_double_option;

    #[derive(Debug, Deserialize, Default, PartialEq)]
    struct Patch {
        #[serde(default, deserialize_with = "deserialize_double_option")]
        field: Option<Option<i32>>,
    }

    #[test]
    fn absent_key_is_outer_none() {
        let patch: Patch = serde_json::from_str("{}").unwrap();
        assert_eq!(patch.field, None);
    }

    #[test]
    fn explicit_null_is_some_none() {
        let patch: Patch = serde_json::from_str(r#"{"field": null}"#).unwrap();
        assert_eq!(patch.field, Some(None));
    }

    #[test]
    fn explicit_value_is_some_some() {
        let patch: Patch = serde_json::from_str(r#"{"field": 7}"#).unwrap();
        assert_eq!(patch.field, Some(Some(7)));
    }
}

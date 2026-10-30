//! [`Challenges`]: the `WWW-Authenticate` values of an error answer.

use std::ops::Deref;

/// Every `WWW-Authenticate` value an error answer carried, raw and in the
/// order they arrived (cratestack#1243): an `AuthRefusal`'s challenges.
/// Unparsed: a challenge's own grammar (RFC 9110 §11.6.1) is the caller's.
///
/// Reads as a `[String]` (it derefs to one). One pointer wide, and empty
/// without allocating, so it keeps `ClientError` and `RpcClientError` small
/// (`clippy::result_large_err`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(
    clippy::box_collection,
    reason = "a thin pointer: `Vec` or `Box<[_]>` would push `RpcClientError` past `result_large_err`"
)]
pub struct Challenges(Option<Box<Vec<String>>>);

impl Challenges {
    /// The values as a slice; empty when the answer had none.
    pub fn as_slice(&self) -> &[String] {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }
}

impl From<Vec<String>> for Challenges {
    fn from(values: Vec<String>) -> Self {
        Self((!values.is_empty()).then(|| Box::new(values)))
    }
}

impl Deref for Challenges {
    type Target = [String];

    fn deref(&self) -> &[String] {
        self.as_slice()
    }
}

impl<'a> IntoIterator for &'a Challenges {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

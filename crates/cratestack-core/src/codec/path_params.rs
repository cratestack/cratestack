//! [`PathParams`]: the matched REST path parameter values a [`Binding`]
//! carries (ADR 0006 §4, as amended on 2026-09-24).
//!
//! [`Binding`]: super::Binding

/// The path parameter values the router matched, in route-template order,
/// as it decoded them. Empty for RPC. See [`Binding::path_params`].
///
/// This is a small `Cow`-like enum instead of `Cow<'a, [Cow<'a, str>]>`.
/// `Cow` names its owned form through `<B as ToOwned>::Owned`, and a
/// lifetime inside that projection makes [`Binding`] **invariant** over
/// `'a`. The trait takes `&'a Binding<'a>`, so an invariant `Binding` would
/// stop a generic caller from passing a `&Binding<'_>` it borrowed for a
/// shorter time than the binding's own lifetime. Here the borrowed form is
/// a plain `&'a [&'a str]`, which keeps `Binding` covariant. It also lets
/// generated code, which knows a route's parameter count, bind the values
/// from a stack array without allocating.
///
/// Equality compares the values, not the variant, the way `Cow`'s does.
///
/// [`Binding`]: super::Binding
/// [`Binding::path_params`]: super::Binding::path_params
#[derive(Debug, Clone)]
pub enum PathParams<'a> {
    /// Borrowed from the router's matched parameters. `Borrowed(&[])`,
    /// which [`PathParams::EMPTY`] and `Default` return, allocates nothing.
    Borrowed(&'a [&'a str]),
    /// Owned values, for a `Binding<'static>` that outlives the request.
    Owned(Vec<String>),
}

impl PathParams<'_> {
    /// No parameters: every RPC binding, and a REST route without any.
    pub const EMPTY: PathParams<'static> = PathParams::Borrowed(&[]);

    pub fn len(&self) -> usize {
        match self {
            PathParams::Borrowed(values) => values.len(),
            PathParams::Owned(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The values in template order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        (0..self.len()).map(move |index| match self {
            PathParams::Borrowed(values) => values[index],
            PathParams::Owned(values) => values[index].as_str(),
        })
    }

    /// Detach from the request. Allocates one `String` per value, and
    /// nothing when there are none (an empty `Vec` does not allocate).
    pub fn into_owned(self) -> PathParams<'static> {
        match self {
            PathParams::Borrowed(values) => {
                PathParams::Owned(values.iter().map(|value| (*value).to_owned()).collect())
            }
            PathParams::Owned(values) => PathParams::Owned(values),
        }
    }
}

impl Default for PathParams<'_> {
    fn default() -> Self {
        PathParams::Borrowed(&[])
    }
}

impl PartialEq for PathParams<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl Eq for PathParams<'_> {}

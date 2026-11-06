//! [`PrincipalMapper`]: which mapping `SignedRequestAuthProvider` applies
//! to a verified principal (ADR 0021 §2, item 3).

use std::fmt;
use std::sync::Arc;

use cratestack_core::{CratestackContext, CratestackError};

use crate::id_token::RequestPrincipal;

use super::{principal_to_contract_context, principal_to_cratestack_context};

type MapFn = dyn Fn(&RequestPrincipal, Option<&str>, bool) -> Result<CratestackContext, CratestackError>
    + Send
    + Sync;

/// Maps a verified [`RequestPrincipal`] to a [`CratestackContext`]. The
/// arguments are the principal, the default role for a service caller, and
/// whether a service caller (no id token) is admitted at all.
///
/// The default is [`Self::product`], today's shape plus the contract names
/// (the ADR's Q2 recommendation, for one release).
#[derive(Clone)]
pub struct PrincipalMapper {
    map: Arc<MapFn>,
    name: &'static str,
}

impl PrincipalMapper {
    /// [`principal_to_cratestack_context`]: the product shape, plus `sub`,
    /// `roles` and `cnf`.
    pub fn product() -> Self {
        Self {
            map: Arc::new(principal_to_cratestack_context),
            name: "product",
        }
    }

    /// [`principal_to_contract_context`]: the contract names, plus the
    /// product claims named in `selected`.
    pub fn contract<I, S>(selected: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let selected: Vec<String> = selected.into_iter().map(Into::into).collect();
        Self {
            map: Arc::new(move |principal, role, allow| {
                principal_to_contract_context(principal, role, allow, &selected)
            }),
            name: "contract",
        }
    }

    /// A mapping of the application's own.
    pub fn custom<F>(map: F) -> Self
    where
        F: Fn(&RequestPrincipal, Option<&str>, bool) -> Result<CratestackContext, CratestackError>
            + Send
            + Sync
            + 'static,
    {
        Self {
            map: Arc::new(map),
            name: "custom",
        }
    }

    /// Apply the mapping.
    pub fn map(
        &self,
        principal: &RequestPrincipal,
        role: Option<&str>,
        allow_transport_caller: bool,
    ) -> Result<CratestackContext, CratestackError> {
        (self.map)(principal, role, allow_transport_caller)
    }
}

impl Default for PrincipalMapper {
    fn default() -> Self {
        Self::product()
    }
}

impl fmt::Debug for PrincipalMapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PrincipalMapper").field(&self.name).finish()
    }
}

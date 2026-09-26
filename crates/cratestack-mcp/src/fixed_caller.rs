//! The one caller a stdio server runs as, and the rule it must meet.
//!
//! ADR 0002 Q1 makes the application name that caller: `StdioServer::new`
//! and `McpServer::new` take a [`CratestackContext`] with no default. The
//! signature alone still let an application name *nobody*:
//! `CratestackContext` derives `Default` and `CratestackContext::anonymous()`
//! is public. The maintainer decided (cratestack#1033, answering #1071's
//! second question) that an anonymous context is refused when the server is
//! built, so "stdio has no implicit identity" holds at runtime as well as in
//! the type.
//!
//! "Anonymous" is the rule the Streamable HTTP guard applies to what an
//! `AuthProvider` returns (`streamable_http/auth.rs`): a context that is not
//! [`CratestackContext::is_authenticated`]. A `SystemContext` is
//! authenticated, so a service identity is accepted, as is
//! `CratestackContext::authenticated(...)` from a credential the application
//! verified.

use std::fmt;

use cratestack_core::CratestackContext;

use crate::listing::ToolTableError;
use crate::streamable_http::caller::Caller;

/// Why [`crate::StdioServer::new`] or [`crate::McpServer::new`] refused to
/// build a server.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StdioConfigError {
    /// The context names no caller: it is not authenticated and not a
    /// `SystemContext`. Name one deliberately instead.
    AnonymousContext,
    /// The generated tool table is not servable (see [`ToolTableError`]).
    Tools(ToolTableError),
}

impl fmt::Display for StdioConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AnonymousContext => formatter.write_str(
                "mcp stdio: the caller's context is anonymous; pass \
                 `SystemContext::for_service(..).into_context()` or \
                 `CratestackContext::authenticated(..)` built from a credential you verified",
            ),
            Self::Tools(error) => write!(formatter, "mcp stdio: {error}"),
        }
    }
}

impl std::error::Error for StdioConfigError {}

impl From<ToolTableError> for StdioConfigError {
    fn from(error: ToolTableError) -> Self {
        Self::Tools(error)
    }
}

impl Caller {
    /// The stdio caller, or a refusal when `context` is anonymous.
    pub(crate) fn fixed(context: CratestackContext) -> Result<Self, StdioConfigError> {
        if !context.is_authenticated() {
            return Err(StdioConfigError::AnonymousContext);
        }
        Ok(Self::Fixed(Box::new(context)))
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::{CratestackContext, SystemContext, Value};

    use super::StdioConfigError;
    use crate::streamable_http::caller::Caller;

    #[test]
    fn an_anonymous_context_is_refused_however_it_was_built() {
        for context in [CratestackContext::anonymous(), CratestackContext::default()] {
            assert_eq!(
                Caller::fixed(context).err(),
                Some(StdioConfigError::AnonymousContext)
            );
        }
    }

    #[test]
    fn a_service_or_a_verified_user_is_a_caller() {
        let user = CratestackContext::authenticated([("id".to_owned(), Value::String("u".into()))]);
        let service = SystemContext::for_service("svc").into_context();
        for context in [user, service] {
            assert!(matches!(Caller::fixed(context), Ok(Caller::Fixed(_))));
        }
    }
}

//! The default way to name a sync principal: the principal contract's
//! `subject()` (ADR 0021 §2), the same "who" the audit actor and MCP's
//! admission key read.

use cratestack_core::CratestackContext;

use super::SyncPrincipal;

impl SyncPrincipal {
    /// The principal for `ctx`, identified by `ctx.subject()` (`sub`, then
    /// `actor.id`, then `id`, an integer as its decimal digits), with the
    /// scopes `scopes` derives from that id. `None` (sync refused) when the
    /// context has no subject, or an empty one.
    pub fn from_subject(
        ctx: &CratestackContext,
        scopes: impl FnOnce(&str) -> Vec<String>,
    ) -> Option<Self> {
        let id = ctx.subject()?.into_owned();
        if id.is_empty() {
            return None;
        }
        let scopes = scopes(&id);
        Some(Self { id, scopes })
    }
}

/// A resolver for [`super::SyncServer::builder`] that identifies the caller
/// by `subject()` and derives its scopes from that id:
///
/// ```text
/// SyncServer::builder(pool, registry, subject_resolver(|sub| vec![format!("user:{sub}")]))
/// ```
///
/// A deployment whose scopes need more than the id (a membership lookup)
/// writes its own resolver and can still call [`SyncPrincipal::from_subject`].
pub fn subject_resolver<F>(
    scopes: F,
) -> impl Fn(&CratestackContext) -> Option<SyncPrincipal> + Send + Sync + 'static
where
    F: Fn(&str) -> Vec<String> + Send + Sync + 'static,
{
    move |ctx| SyncPrincipal::from_subject(ctx, &scopes)
}

#[cfg(test)]
mod tests {
    use cratestack_core::{CratestackContext, Value};

    use super::*;

    fn user_scope(sub: &str) -> Vec<String> {
        vec![format!("user:{sub}")]
    }

    #[test]
    fn the_default_resolver_reads_the_subject() {
        let resolve = subject_resolver(user_scope);
        let sub = CratestackContext::builder("u-1")
            .claim("id", Value::String("legacy".to_owned()))
            .build();
        assert_eq!(
            resolve(&sub),
            Some(SyncPrincipal {
                id: "u-1".to_owned(),
                scopes: vec!["user:u-1".to_owned()],
            })
        );
        let int_id = CratestackContext::authenticated([("id".to_owned(), Value::Int(7))]);
        assert_eq!(resolve(&int_id).map(|p| p.id), Some("7".to_owned()));
        assert_eq!(resolve(&CratestackContext::anonymous()), None);
        let empty = CratestackContext::builder("").build();
        assert_eq!(resolve(&empty), None);
    }
}

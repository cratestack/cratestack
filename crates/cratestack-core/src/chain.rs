//! The first-success provider chain (ADR 0021 §5, cratestack#1243):
//! several [`AuthProvider`]s composed into one, in order.
//!
//! ```text
//! let auth = first_success(oauth).or(device_signature).or(cose_signer);
//! ```
//!
//! For each member, in order:
//!
//! 1. If its [`credential`](AuthProvider::credential) is
//!    [`Credential::Absent`], it is skipped without being called.
//! 2. If it returns an authenticated context, that is the answer.
//! 3. If it returns an error (an [`AuthRefusal`](crate::AuthRefusal) or any
//!    other), that is the answer. **A credential one member refuses is never
//!    rescued by another**: the chain fails closed.
//! 4. If it returns an anonymous context, the next member is asked.
//!
//! When no member authenticates, the chain answers with the first anonymous
//! context a member returned (so what a provider put in it, such as its
//! extensions, is kept), or a plain anonymous context when every member was
//! skipped, and the schema's policies decide. A request that carries two credentials is
//! resolved by order: the first member that authenticates it wins.
//!
//! The chain is generic and static (`Or<Or<A, B>, C>`): `AuthProvider`
//! returns `impl Future` and is not object-safe, so there is no `dyn` list.

use crate::context::{AuthProvider, CratestackContext, RequestContext};
use crate::credential::Credential;
use crate::error::CratestackError;

/// Starts a first-success chain with `first`; add members with
/// [`FirstSuccess::or`]. See the module documentation.
pub fn first_success<P: AuthProvider>(first: P) -> FirstSuccess<P> {
    FirstSuccess(first)
}

/// A first-success chain of [`AuthProvider`]s, itself an `AuthProvider`.
/// Built with [`first_success`].
#[derive(Debug, Clone)]
pub struct FirstSuccess<P>(P);

/// Two chain members, `first` asked before `next`.
#[derive(Debug, Clone)]
pub struct Or<A, B> {
    first: A,
    next: B,
}

impl<P: AuthProvider> FirstSuccess<P> {
    /// Adds `next` after every member already in the chain.
    pub fn or<Q: AuthProvider>(self, next: Q) -> FirstSuccess<Or<P, Q>> {
        FirstSuccess(Or {
            first: self.0,
            next,
        })
    }
}

/// What one member said.
enum Step {
    /// Its credential is absent: it was not called.
    Skipped,
    /// It answered anonymous, with this context.
    Anonymous(CratestackContext),
    /// It authenticated or failed: the chain's answer.
    Done(Result<CratestackContext, CratestackError>),
}

/// Asks one member.
async fn ask<P: AuthProvider>(provider: &P, request: &RequestContext<'_>) -> Step {
    if provider.credential(request).is_absent() {
        return Step::Skipped;
    }
    match provider.authenticate(request).await {
        Ok(context) if context.is_authenticated() => Step::Done(Ok(context)),
        Ok(context) => Step::Anonymous(context),
        Err(error) => Step::Done(Err(error.into())),
    }
}

/// The chain's answer once every member was asked: the first anonymous
/// context, else a plain anonymous one.
fn settle(steps: [Step; 2]) -> Result<CratestackContext, CratestackError> {
    let mut anonymous = None;
    for step in steps {
        match step {
            Step::Done(answer) => return answer,
            Step::Anonymous(context) => {
                anonymous.get_or_insert(context);
            }
            Step::Skipped => {}
        }
    }
    Ok(anonymous.unwrap_or_else(CratestackContext::anonymous))
}

/// What a chain of members says about the request's credential: present if
/// any member's is (with the first such member's value to redact), else
/// malformed if any member's is, else unknown if any member cannot tell,
/// else absent.
fn combined<'r>(answers: [Credential<'r>; 2]) -> Credential<'r> {
    if let Some(present) = answers
        .iter()
        .find(|answer| matches!(answer, Credential::Present { .. }))
    {
        return *present;
    }
    if let Some(malformed) = answers
        .iter()
        .find(|answer| matches!(answer, Credential::Malformed(_)))
    {
        return *malformed;
    }
    if answers.contains(&Credential::Unknown) {
        return Credential::Unknown;
    }
    Credential::Absent
}

impl<A: AuthProvider, B: AuthProvider> AuthProvider for Or<A, B> {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        let first = ask(&self.first, request).await;
        if let Step::Done(answer) = first {
            return answer;
        }
        settle([first, ask(&self.next, request).await])
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        combined([
            self.first.credential(request),
            self.next.credential(request),
        ])
    }
}

impl<P: AuthProvider> AuthProvider for FirstSuccess<P> {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        settle([ask(&self.0, request).await, Step::Skipped])
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        self.0.credential(request)
    }
}

#[cfg(test)]
#[path = "chain_tests.rs"]
mod tests;

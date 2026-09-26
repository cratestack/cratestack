//! stdio has no implicit identity, at runtime as well as in the signature
//! (ADR 0002 Q1; maintainer decision on cratestack#1033, answering #1071's
//! second question): both public constructors refuse an anonymous context,
//! and accept a service or a verified user.

mod support;

use cratestack_core::{CratestackContext, SystemContext};
use cratestack_mcp::{McpServer, StdioConfigError, StdioServer};
use support::{FakeTools, user, without_id};

#[test]
fn an_anonymous_context_builds_no_server() {
    for context in [CratestackContext::anonymous(), CratestackContext::default()] {
        let stdio = StdioServer::new(FakeTools::default(), context.clone());
        assert_eq!(stdio.err(), Some(StdioConfigError::AnonymousContext));
        let server = McpServer::new(FakeTools::default(), context);
        assert_eq!(server.err(), Some(StdioConfigError::AnonymousContext));
    }
}

#[test]
fn the_refusal_names_what_to_pass_instead() {
    let message = StdioConfigError::AnonymousContext.to_string();
    assert!(message.contains("anonymous"), "{message}");
    assert!(message.contains("SystemContext::for_service"), "{message}");
}

/// Refused is exactly "not authenticated": a context with no `id` claim is
/// still a named caller here. Its keyed and rate-limited calls are refused
/// later, by the admission namespace rule (`tests/admission.rs`).
#[test]
fn a_service_a_user_and_a_user_without_an_id_are_callers() {
    let service = SystemContext::for_service("svc").into_context();
    for context in [service, user("u-1"), without_id()] {
        assert!(StdioServer::new(FakeTools::default(), context).is_ok());
    }
}

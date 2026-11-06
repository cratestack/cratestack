#![cfg(test)]

//! The audit actor's id is the principal contract's `subject()` (ADR 0021
//! §2): `sub`, then `actor.id`, then `id`, an integer as its digits.

use cratestack_core::{CratestackContext, Value};

use crate::audit::actor_from_context;

#[test]
fn the_audit_actor_id_is_the_subject() {
    let sub = CratestackContext::builder("user-42").build();
    assert_eq!(actor_from_context(&sub).id.as_deref(), Some("user-42"));

    // Before the contract, an integer `id` recorded no actor at all.
    let int_id = CratestackContext::authenticated([("id".to_owned(), Value::Int(7))]);
    assert_eq!(actor_from_context(&int_id).id.as_deref(), Some("7"));

    let anonymous = CratestackContext::anonymous();
    assert_eq!(actor_from_context(&anonymous).id, None);
}

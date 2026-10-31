//! The service-caller mapping: a request signed by a trusted service key,
//! with no user id token.

use crate::id_token::RequestPrincipal;

use super::PROOF_OF_POSSESSION;

pub(super) fn service_principal_to_cratestack_context(
    principal: &RequestPrincipal,
    role: Option<&str>,
) -> cratestack_core::CratestackContext {
    let caller_id = format!("svc:{}", principal.transport.key_id);
    let role = role.unwrap_or("caller");
    cratestack_core::CratestackContext::authenticated([
        (
            "sub".to_owned(),
            cratestack_core::Value::String(caller_id.clone()),
        ),
        (
            "roles".to_owned(),
            cratestack_core::Value::List(vec![cratestack_core::Value::String(role.to_owned())]),
        ),
        // ADR 0022: the request was signed with the service's key.
        (
            "amr".to_owned(),
            cratestack_core::Value::List(vec![cratestack_core::Value::String(
                PROOF_OF_POSSESSION.to_owned(),
            )]),
        ),
        (
            "id".to_owned(),
            cratestack_core::Value::String(caller_id.clone()),
        ),
        (
            "clientId".to_owned(),
            cratestack_core::Value::String(caller_id),
        ),
        (
            "enrollmentStatus".to_owned(),
            cratestack_core::Value::String("trusted_signature".to_owned()),
        ),
        (
            "boundKeyId".to_owned(),
            cratestack_core::Value::String(principal.transport.key_id.clone()),
        ),
        (
            "role".to_owned(),
            cratestack_core::Value::String(role.to_owned()),
        ),
        (
            "callerService".to_owned(),
            cratestack_core::Value::String(principal.transport.key_id.clone()),
        ),
        (
            "serviceName".to_owned(),
            cratestack_core::Value::String(principal.transport.key_id.clone()),
        ),
        (
            "actorType".to_owned(),
            cratestack_core::Value::String("service".to_owned()),
        ),
    ])
}

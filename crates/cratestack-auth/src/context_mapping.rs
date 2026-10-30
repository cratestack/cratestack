//! Mapping a verified [`RequestPrincipal`] onto a
//! [`cratestack_core::CratestackContext`].
//!
//! Two mappings (ADR 0021 §2, cratestack#1243). The product one,
//! [`principal_to_cratestack_context`], keeps the shape Webank's policies
//! read (`auth().actor.id`, `kycStatus`, `session.boundKeyId`) and also
//! emits the principal contract's names it knows: `sub`, `roles` and `cnf`.
//! The contract one, [`principal_to_contract_context`], emits only the
//! contract's names plus the free-form claims a deployment selects.
//! [`PrincipalMapper`] picks one for `SignedRequestAuthProvider`.

mod contract;
mod mapper;
#[cfg(test)]
mod tests;

use serde::Serialize;

use crate::id_token::RequestPrincipal;

pub use contract::principal_to_contract_context;
pub use mapper::PrincipalMapper;

/// The product mapping: today's claims, plus the contract's `sub` (the user
/// id, or `svc:<key id>` for a service caller), `roles` (the one verified
/// `role`) and, for a user, `cnf` (`kid` the bound key's id, `jkt` its RFC
/// 7638 thumbprint when the id token carried the key).
pub fn principal_to_cratestack_context(
    principal: &RequestPrincipal,
    role: Option<&str>,
    allow_transport_caller: bool,
) -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError> {
    let Some(user) = principal.user.as_ref() else {
        if allow_transport_caller {
            return Ok(service_principal_to_cratestack_context(principal, role));
        }
        return Ok(cratestack_core::CratestackContext::anonymous());
    };

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ActorPrincipal {
        id: String,
        enrollment_status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        kyc_status: Option<String>,
        profile_version: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        main_email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        main_phone: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        main_address: Option<serde_json::Value>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SessionPrincipal {
        client_id: String,
        audience: String,
        bound_key_id: String,
        request_key_id: String,
        request_nonce: String,
        request_timestamp: String,
        issued_at: i64,
        expires_at: i64,
    }

    #[derive(Serialize)]
    struct Confirmation {
        kid: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        jkt: Option<String>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CratestackPrincipal {
        sub: String,
        roles: Vec<String>,
        cnf: Confirmation,
        actor: ActorPrincipal,
        session: SessionPrincipal,
        id: String,
        client_id: String,
        enrollment_status: String,
        bound_key_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        kyc_status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        kyc_dossier_id: Option<String>,
    }

    let kyc_dossier_id = user
        .disclosed_claims
        .get("kycDossierId")
        .and_then(|value| value.as_str().map(str::to_owned));

    cratestack_core::CratestackContext::from_principal(Some(CratestackPrincipal {
        sub: user.user_id.clone(),
        roles: vec![user.role.clone()],
        cnf: Confirmation {
            kid: user.bound_key_id.clone(),
            jkt: user.bound_key_jkt.clone(),
        },
        actor: ActorPrincipal {
            id: user.user_id.clone(),
            enrollment_status: user.enrollment_status.clone(),
            kyc_status: user.kyc_status.clone(),
            profile_version: user.profile_version,
            main_email: user.main_email.clone(),
            main_phone: user.main_phone.clone(),
            main_address: user.main_address.clone(),
        },
        session: SessionPrincipal {
            client_id: user.client_id.clone(),
            audience: user.audience.clone(),
            bound_key_id: user.bound_key_id.clone(),
            request_key_id: principal.transport.key_id.clone(),
            request_nonce: principal.transport.nonce.clone(),
            request_timestamp: principal.transport.timestamp.to_rfc3339(),
            issued_at: user.issued_at,
            expires_at: user.expires_at,
        },
        id: user.user_id.clone(),
        client_id: user.client_id.clone(),
        enrollment_status: user.enrollment_status.clone(),
        bound_key_id: user.bound_key_id.clone(),
        // A user principal's role comes from the *verified* id_jwt `role`
        // claim, NOT the caller-supplied `role` argument. The argument only
        // names the default role for the service-caller path below. This is
        // what makes admin server-backed: the issuer stamps `role` from
        // `User.isAdmin`, so a caller can't self-grant via `client_id`/`azp`.
        role: Some(user.role.clone()),
        kyc_status: user.kyc_status.clone(),
        kyc_dossier_id,
    }))
}

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
        (
            "id".to_owned(),
            cratestack_core::Value::String(caller_id.clone()),
        ),
        (
            "clientId".to_owned(),
            cratestack_core::Value::String(caller_id.clone()),
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

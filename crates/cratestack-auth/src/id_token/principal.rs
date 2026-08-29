use std::collections::HashMap;

use serde_json::Value;

use crate::SignedRequestPrincipal;

#[derive(Clone, Debug, PartialEq)]
pub struct UserPrincipal {
    pub user_id: String,
    pub audience: String,
    pub client_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub bound_key_id: String,
    pub profile_version: i32,
    pub enrollment_status: String,
    pub kyc_status: Option<String>,
    /// Verified authorization role from the id_jwt `role` claim. Source of
    /// truth for privileged-access gating across services.
    pub role: String,
    pub main_email: Option<String>,
    pub main_phone: Option<String>,
    pub main_address: Option<Value>,
    /// Claims the holder disclosed alongside this token. Only populated when the
    /// presented compact form was an SD-JWT (`<jwt>~d1~d2~`) and each disclosure
    /// hashed back to a digest in `_sd[]`.
    pub disclosed_claims: HashMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RequestPrincipal {
    pub transport: SignedRequestPrincipal,
    pub user: Option<UserPrincipal>,
}

#[derive(Clone, Debug)]
pub struct CurrentPrincipal(pub RequestPrincipal);

#[derive(Clone, Debug)]
pub struct AuthenticatedPrincipal(pub RequestPrincipal);

impl CurrentPrincipal {
    pub fn user(&self) -> Option<&UserPrincipal> {
        self.0.user.as_ref()
    }
}

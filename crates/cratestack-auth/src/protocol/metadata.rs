//! Authorization-server metadata and JWKS documents.

use serde::{Deserialize, Serialize};

use crate::{ID_TOKEN_GRANT, REFRESH_TOKEN_GRANT};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub userinfo_endpoint: String,
    pub introspection_endpoint: String,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JwksDocument {
    pub keys: Vec<Jwk>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Jwk {
    pub kty: String,
    pub kid: String,
    pub alg: String,
    #[serde(rename = "use")]
    pub key_use: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
}

pub fn authorization_server_metadata(issuer: &str) -> AuthorizationServerMetadata {
    AuthorizationServerMetadata {
        issuer: issuer.to_string(),
        token_endpoint: format!("{issuer}/token"),
        jwks_uri: format!("{issuer}/jwks.json"),
        userinfo_endpoint: format!("{issuer}/userinfo"),
        introspection_endpoint: format!("{issuer}/introspect"),
        grant_types_supported: vec![ID_TOKEN_GRANT.to_string(), REFRESH_TOKEN_GRANT.to_string()],
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
        response_types_supported: Vec::new(),
    }
}

pub fn jwks(keys: Vec<Jwk>) -> JwksDocument {
    JwksDocument { keys }
}

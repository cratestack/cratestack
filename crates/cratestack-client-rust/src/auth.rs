use crate::error::ClientError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub method: String,
    pub path: String,
    pub canonical_query: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub canonical_request: String,
}

pub trait RequestAuthorizer: Send + Sync {
    fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError>;
}

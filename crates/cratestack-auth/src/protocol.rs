//! Wire types for the auth protocol surface — the authorization-server
//! metadata/JWKS documents, the device enrolment exchange, and the
//! token/userinfo/introspection exchange.
//!
//! These are plain serde DTOs with no behaviour beyond the two small
//! constructors in [`metadata`]; they are grouped by exchange rather than
//! by shape so a reader looking for "what does `/token` return" finds the
//! whole exchange in one file.

mod enroll;
mod metadata;
mod token;

pub use enroll::{
    DeviceSummary, EnrollRequest, EnrollResponse, KeySummary, NextStep, UserSummary, VerifyRequest,
    VerifyResponse,
};
pub use metadata::{
    AuthorizationServerMetadata, Jwk, JwksDocument, authorization_server_metadata, jwks,
};
pub use token::{
    Confirmation, IntrospectRequest, IntrospectResponse, TokenRequest, TokenResponse,
    UserinfoResponse,
};

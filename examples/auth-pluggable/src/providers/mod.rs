//! The three providers. Each is one file, built only on the public
//! `AuthProvider` seam (ADR 0021): no provider reaches anything private in
//! CrateStack, and nothing else in the example knows which one runs.

pub mod devsig;
pub mod inhouse;
pub mod oidc;

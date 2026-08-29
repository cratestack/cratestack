//! [`SignedRequestVerifier`]: construction/configuration lives in
//! [`config`], the actual verify/authenticate request path in
//! [`authenticate`].

mod authenticate;
mod config;

pub use config::SignedRequestVerifier;

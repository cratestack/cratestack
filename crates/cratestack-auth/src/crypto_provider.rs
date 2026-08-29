//! Process-wide `rustls` crypto-provider fallback.

/// Installs a `ring`-backed `rustls::crypto::CryptoProvider` if the process
/// doesn't already have one. `reqwest`'s `rustls-no-provider` feature (see
/// the workspace `Cargo.toml`'s `reqwest` entry) ships no crypto provider at
/// all: `reqwest::Client::build()` PANICS at construction time if
/// `rustls::crypto::CryptoProvider::get_default()` finds nothing installed.
/// [`IdTokenVerifier::new`][crate::IdTokenVerifier::new] and
/// [`MultiIssuerJwksVerifier::new`][crate::MultiIssuerJwksVerifier::new] both
/// build a `reqwest::Client` internally, so this crate needs the same fallback
/// `cratestack-client-rust::client::core::ensure_crypto_provider` installs,
/// for the identical reason.
///
/// `install_default()` only ever takes effect the FIRST time it succeeds
/// process-wide — it's a courtesy fallback, not an override. A consumer that
/// installs its own provider (any backend, including `aws-lc-rs`) before
/// constructing its first verifier keeps that choice; this only fires when
/// nobody has chosen anything yet. The `Err` it returns on a race with
/// another caller installing first (or a no-op call to this same function
/// from a second verifier construction) is expected and intentionally
/// ignored.
pub(crate) fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

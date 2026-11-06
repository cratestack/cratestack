//! The generated Rust client for `schema.cstack`, sealed and compact.

use cratestack::cose::{CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver};
use cratestack_client_rust::{CborCodec, ClientConfig, ClientEnvelope, CratestackClient};
use std::sync::Arc;
use url::Url;

cratestack::include_client_schema!("../schema.cstack");

pub use cratestack_schema as schema;

/// The audience the server's envelope is addressed to.
pub const AUDIENCE: &str = "payments";

/// DEMO SECRET, FOR LOCAL USE ONLY: the same published bytes the server
/// example trusts. Never ship a secret in source.
pub const DEMO_SECRET: &[u8; 32] = b"signed-compact-DEMO-ONLY-secret!";

/// A client for the server at `base`: COSE Mac0 sealed (`with_envelope`) and,
/// when `compact` is set, opted in to compact wire keys (`with_compact_keys`).
pub fn client(base: Url, secret: &[u8], compact: bool) -> schema::client::Client {
    let hmac = || HmacSigner::new(CoseAlg::Hmac256_64, secret.to_vec()).expect("a 32-byte secret");
    let cose = CoseEnvelope::client(
        CoseMode::Mac0,
        Arc::new(hmac()),
        Arc::new(StaticVerifierResolver::new().with_key(hmac().verify_key())),
    )
    .build()
    .expect("client envelope");
    let envelope = ClientEnvelope::new(cose, AUDIENCE).expect("client envelope");
    let runtime = CratestackClient::new(ClientConfig::new(base), CborCodec)
        .with_envelope(envelope)
        .expect("a CBOR client takes an envelope");
    if compact {
        schema::client::Client::with_compact_keys(runtime)
    } else {
        schema::client::Client::new(runtime)
    }
}

/// The transfer both calls send.
pub fn transfer() -> schema::procedures::transfer_funds::Args {
    schema::procedures::transfer_funds::Args {
        input: schema::TransferInput {
            fromAccount: "acct-checking-001".to_owned(),
            toAccount: "acct-savings-002".to_owned(),
            amountMinor: 12_500,
            currency: "EUR".to_owned(),
            reference: "invoice-2026-0417".to_owned(),
            memo: Some("April rent".to_owned()),
        },
    }
}

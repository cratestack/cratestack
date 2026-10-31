//! The service: `schema.cstack` over REST and `schema-rpc.cstack` over RPC,
//! one procedure registry each, behind CrateStack's in-house provider.
//!
//! The provider is `SignedRequestAuthProvider` with an id token verifier and
//! no key registry: a request is signed by a device key the id token vouches
//! for (`cnf.jwk`). Its product mapping copies the token's `auth_time`, and
//! its `amr` plus `pop` (the request proved the key), onto the principal
//! contract (ADR 0021 §2, ADR 0022). Nothing here checks freshness or
//! methods: the generated code does, from `@authn`.

use std::sync::{Arc, Mutex};

use cratestack::axum::Router;
use cratestack::{CratestackContext, CratestackError};
use cratestack_auth::{
    AuthError, IdTokenVerifier, SignedRequestAuthProvider, SignedRequestVerifier, jwks_router,
};
use cratestack_codec_cbor::CborCodec;

use crate::issuer::{ISSUER, Issuer};

/// The REST binding of the schema.
pub mod rest {
    cratestack::include_server_schema!("schema.cstack", db = None);
}

/// The RPC binding of the same schema.
pub mod rpc {
    cratestack::include_server_schema!("schema-rpc.cstack", db = None);
}

/// Every invoice paid, in order: a step-up refusal happens before the
/// procedure runs, so a refused payment never appears here and a retried one
/// appears once.
#[derive(Clone, Default)]
pub struct Ledger(Arc<Mutex<Vec<String>>>);

impl Ledger {
    /// The invoice ids paid so far.
    pub fn paid(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn record(&self, invoice_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(invoice_id.to_owned());
    }
}

fn subject(ctx: &CratestackContext) -> String {
    ctx.subject().map(String::from).unwrap_or_default()
}

macro_rules! procedures {
    ($schema:ident) => {
        impl $schema::cratestack_schema::procedures::ProcedureRegistry for Ledger {
            async fn pay_invoice(
                &self,
                _db: &$schema::cratestack_schema::Cratestack,
                ctx: &CratestackContext,
                args: $schema::cratestack_schema::procedures::pay_invoice::Args,
                _authorized: $schema::cratestack_schema::procedures::pay_invoice::Authorized,
            ) -> Result<$schema::cratestack_schema::Receipt, CratestackError> {
                self.record(&args.args.invoiceId);
                Ok($schema::cratestack_schema::Receipt {
                    invoiceId: args.args.invoiceId,
                    amountCents: args.args.amountCents,
                    paidBy: subject(ctx),
                })
            }

            async fn balance(
                &self,
                _db: &$schema::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $schema::cratestack_schema::procedures::balance::Args,
                _authorized: $schema::cratestack_schema::procedures::balance::Authorized,
            ) -> Result<$schema::cratestack_schema::Balance, CratestackError> {
                Ok($schema::cratestack_schema::Balance {
                    account: args.args.account,
                    cents: 12_500,
                })
            }
        }
    };
}

procedures!(rest);
procedures!(rpc);

/// The in-house provider, verifying id tokens against the JWKS at
/// `jwks_url`.
pub fn provider(jwks_url: &str) -> Result<SignedRequestAuthProvider, AuthError> {
    let tokens = IdTokenVerifier::new(ISSUER, jwks_url, None)?;
    let verifier = SignedRequestVerifier::new(Vec::new()).with_id_token_verifier(tokens);
    Ok(SignedRequestAuthProvider::new(verifier))
}

/// The whole service: REST at `/$procs/...`, RPC at `/rpc/...`, and the
/// issuer's keys at `/.well-known/jwks.json` (which `provider` fetches).
/// One process serves both here to keep the example to one binary; in a
/// deployment the JWKS is the auth service's.
pub fn app(issuer: &Issuer, ledger: Ledger, jwks_url: &str) -> Result<Router, AuthError> {
    let auth = provider(jwks_url)?;
    let rest = rest::cratestack_schema::axum::router(
        rest::cratestack_schema::Cratestack::builder().build(),
        ledger.clone(),
        (),
        CborCodec,
        auth.clone(),
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let rpc = rpc::cratestack_schema::axum::rpc_router(
        rpc::cratestack_schema::Cratestack::builder().build(),
        ledger,
        (),
        CborCodec,
        auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    Ok(rest.merge(rpc).merge(jwks_router(issuer.jwks())))
}

/// Binds `addr`, serves [`app`] on it in the background, and returns the
/// address it listens on.
pub async fn spawn(
    issuer: &Issuer,
    ledger: Ledger,
    addr: &str,
) -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let jwks_url = format!("http://{local}/.well-known/jwks.json");
    let app = app(issuer, ledger, &jwks_url).map_err(std::io::Error::other)?;
    let server = tokio::spawn(async move {
        let _ = cratestack::axum::serve(listener, app).await;
    });
    Ok((local, server))
}

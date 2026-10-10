//! A COSE-signed server for the Dart client's live round trip
//! (cratestack#1026): `dart-packages/cratestack_cbor/test/cose/live_test.dart`
//! spawns it, seals requests with `cratestack_cbor`, and opens the answers.
//!
//! One `procedure echo`, served twice: REST (`POST /$procs/echo`) and
//! `transport rpc` (`POST /rpc/procedure.echo`), each behind its own
//! envelope layer in `Required` mode, with an in-memory nonce store.
//!
//! ```text
//! cargo run -p cratestack-api --features cose --example cose_roundtrip_server -- --mode sign1
//! contract POST /$procs/echo <64 hex>
//! contract POST procedure.echo <64 hex>
//! listening 38417
//! ```
//!
//! `--mode sign1` (default) trusts the published Ed25519 test key
//! `ed25519` of `crates/cratestack-cose/tests/vectors/keys.json` and signs
//! with `ed25519_other`; `--mode mac0` shares the vectors' HMAC secret.
//! The audience is `roundtrip`. The keys are published in this repository:
//! **never use them anywhere else.** The `contract` lines give the client
//! the op digests a generated client would bake in; `listening` comes last.

use std::sync::Arc;

use cratestack::InMemoryNonceStore;
use cratestack::cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseSigner, Ed25519Signer, HmacSigner, StaticVerifierResolver,
};

const AUDIENCE: &str = "roundtrip";

/// One schema, one module: the generated server, the `echo` procedure, a
/// router behind its own envelope layer, and the digest of the op.
macro_rules! echo_server {
    ($module:ident, $schema:literal, $router:ident) => {
        mod $module {
            use cratestack::axum::Router;
            use cratestack::cose::CoseEnvelope;
            use cratestack::envelope_layer::EnvelopeMode;
            use cratestack::{CratestackContext, CratestackError, VerifiedSigner};
            use cratestack_codec_cbor::CborCodec;

            cratestack::include_server_schema!($schema, db = None);

            use cratestack_schema as srv;

            #[derive(Clone)]
            struct Echo;

            impl srv::procedures::ProcedureRegistry for Echo {
                fn echo(
                    &self,
                    _db: &srv::Cratestack,
                    ctx: &CratestackContext,
                    args: srv::procedures::echo::Args,
                    _authorized: srv::procedures::echo::Authorized,
                ) -> impl core::future::Future<
                    Output = Result<srv::procedures::echo::Output, CratestackError>,
                > + Send {
                    let signer = ctx.verified_signer().map(VerifiedSigner::alg);
                    async move {
                        Ok(srv::EchoReply {
                            message: args.args.message,
                            signer: format!("{signer:?}"),
                        })
                    }
                }
            }

            #[derive(Clone)]
            struct AllowAll;

            impl cratestack::AuthProvider for AllowAll {
                type Error = CratestackError;

                fn authenticate(
                    &self,
                    _request: &cratestack::RequestContext<'_>,
                ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send
                {
                    core::future::ready(Ok(CratestackContext::authenticated([(
                        "id".to_owned(),
                        cratestack::Value::Int(1),
                    )])))
                }
            }

            /// The generated router behind the envelope layer, `Required`.
            pub fn router(envelope: CoseEnvelope, audience: &'static str) -> Router {
                srv::axum::$router(
                    srv::Cratestack::builder().build(),
                    Echo,
                    (),
                    CborCodec,
                    AllowAll,
                    cratestack::DEFAULT_BODY_LIMIT_BYTES,
                )
                .layer(
                    srv::axum::envelope_layer(envelope, EnvelopeMode::Required, audience)
                        .build()
                        .expect("envelope layer"),
                )
            }

            /// `echo`'s op contract digest as the generated client binds it.
            pub fn contract(route: &str) -> [u8; 32] {
                *cratestack::find_contract(srv::OP_CONTRACTS, "POST", route).expect("digest")
            }
        }
    };
}

echo_server!(rest, "examples/cose_roundtrip.cstack", router);
echo_server!(rpc, "examples/cose_roundtrip_rpc.cstack", rpc_router);

fn seed(start: u8) -> [u8; 32] {
    std::array::from_fn(|index| start + index as u8)
}

/// The server's envelope for `mode`, with a nonce store of its own.
fn server_envelope(mode: &str) -> CoseEnvelope {
    let (mode, signer, resolver): (_, Arc<dyn CoseSigner>, _) = match mode {
        "sign1" => (
            CoseMode::Sign1,
            Arc::new(Ed25519Signer::from_seed(&seed(0x80))),
            StaticVerifierResolver::new().with_key(Ed25519Signer::from_seed(&seed(0)).verify_key()),
        ),
        "mac0" => {
            let hmac = || HmacSigner::new(CoseAlg::Hmac256_64, seed(0x40).to_vec()).expect("key");
            (
                CoseMode::Mac0,
                Arc::new(hmac()),
                StaticVerifierResolver::new().with_key(hmac().verify_key()),
            )
        }
        other => panic!("--mode is sign1 or mac0, not {other}"),
    };
    CoseEnvelope::server(
        mode,
        signer,
        Arc::new(resolver),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .expect("server envelope")
}

fn contract_line(route: &str, digest: [u8; 32]) {
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    println!("contract POST {route} {hex}");
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let mut mode = "sign1".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => mode = args.next().expect("--mode needs sign1 or mac0"),
            other => panic!("unknown argument {other}"),
        }
    }

    // Each transport has its own envelope, so its own nonce store: a
    // request reaches exactly one of them.
    let app = rest::router(server_envelope(&mode), AUDIENCE)
        .merge(rpc::router(server_envelope(&mode), AUDIENCE));
    contract_line("/$procs/echo", rest::contract("/$procs/echo"));
    contract_line("procedure.echo", rpc::contract("procedure.echo"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    println!("listening {}", listener.local_addr().expect("addr").port());
    cratestack::axum::serve(listener, app).await.expect("serve");
}

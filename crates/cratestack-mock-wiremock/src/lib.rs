//! Generates WireMock stub mappings (<https://wiremock.org/docs/stubbing/>)
//! from a `.cstack` schema's procedures, so integration/e2e tests can run
//! against a mock backend whose wire contract is derived from the same
//! schema the real server is generated from, instead of a hand-maintained
//! JSON fixture that can silently drift from it. See
//! `docs/design/wiremock-stubs.md` for the motivating case, full design,
//! and open questions this crate's v1 slice deliberately leaves open.
//!
//! # Scope (v3 — stateful model CRUD)
//!
//! - Covered: `procedure`/`mutation procedure` declarations (always
//!   static/happy-path — see below), and `model` blocks'
//!   `list`/`get`/`create`/`update`/`delete` CRUD routes.
//! - **`transport rest` model CRUD is stateful.** A record created
//!   through a mocked `create` appears in a subsequent `list`; an
//!   `update` is visible on a subsequent `get`; a `delete`d record's
//!   `get` returns `404`, not a stale body — all backed by a real
//!   per-record store (`wiremock-state-extension`, see
//!   `crate::model_state`'s module doc), not a fixed example replayed
//!   on every request. **This needs more than a plain
//!   `docker run wiremock/wiremock`** — see this crate's `README.md`
//!   and `docs/design/wiremock-stubs.md`'s "Model CRUD statefulness"
//!   section for exactly what, and why a vanilla WireMock instance
//!   cannot do this at all (investigated and ruled out before the
//!   extension-backed path was built). Fields whose type this generator
//!   can't round-trip through the extension's state store (`Optional`/
//!   `List` arity, `Json`/`Bytes`/`Vector`, or a nested `type`) fall
//!   back to a fixed example value, same on every response — see
//!   `crate::model_attrs::ScalarKind`. List filtering
//!   (`field__operator=value`), sorting, and `limit`/`offset` are not
//!   implemented; every `list` response is the complete, unfiltered,
//!   unpaginated collection regardless of query string.
//! - **`transport rpc` model CRUD stays static** (the pre-stateful v1
//!   shape: one deterministic example, replayed identically on every
//!   request) — the extension's per-record context needs something
//!   unique to each request that REST gets for free (the id-bearing URL
//!   path) and RPC doesn't (the id lives in the request body, and this
//!   templating stack has no string-concatenation helper to build a
//!   unique key from it; see `crate::model_mapping::rpc`'s module doc).
//! - Not covered (tracked as follow-ups in the design doc):
//!   `transport grpc` schemas
//!   ([`WireMockGeneratorError::UnsupportedTransport`]), `FindMany<T>`
//!   return types ([`WireMockGeneratorError::UnsupportedReturnType`]),
//!   error-case stubs, request-body assertion, and any emulation of the
//!   auth chokepoint every procedure/model route sits behind.
//!
//! # Example
//!
//! ```
//! let schema = cratestack_parser::parse_schema(
//!     "datasource db {\n  provider = \"none\"\n}\n\n\
//!      type Greeting {\n  message String\n}\n\n\
//!      procedure hello(): Greeting\n",
//! )
//! .expect("schema should parse");
//!
//! let package = cratestack_mock_wiremock::generate_package(
//!     &schema,
//!     &cratestack_mock_wiremock::WireMockGeneratorConfig::default(),
//! )
//! .expect("generation should succeed");
//!
//! assert_eq!(package.files.len(), 1);
//! assert_eq!(package.files[0].file_name, "mappings/hello.json");
//! ```

mod config;
mod error;
mod generator;
mod mapping;
mod model_attrs;
mod model_mapping;
mod model_record;
mod model_state;
mod values;

pub use config::{GeneratedWireMockFile, GeneratedWireMockPackage, WireMockGeneratorConfig};
pub use error::WireMockGeneratorError;
pub use generator::generate_package;

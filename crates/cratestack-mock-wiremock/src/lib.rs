//! Generates WireMock stub mappings (<https://wiremock.org/docs/stubbing/>)
//! from a `.cstack` schema's procedures, so integration/e2e tests can run
//! against a mock backend whose wire contract is derived from the same
//! schema the real server is generated from, instead of a hand-maintained
//! JSON fixture that can silently drift from it. See
//! `docs/design/wiremock-stubs.md` for the motivating case, full design,
//! and open questions this crate's v1 slice deliberately leaves open.
//!
//! # Scope (v2 — model CRUD)
//!
//! - Covered: `procedure`/`mutation procedure` declarations, and `model`
//!   blocks' `list`/`get`/`create`/`update`/`delete` CRUD routes, under
//!   `transport rest` (the schema default) or `transport rpc`.
//!   Happy-path only, and **not stateful** — every generated stub
//!   matches on request method + path (no body assertion) and always
//!   answers with the *same* deterministic example, regardless of what
//!   was previously created/updated/deleted through it. A record
//!   created through a mocked `create` will not appear in a subsequent
//!   `list`, and an updated field will not appear on a subsequent `get`.
//!   See `docs/design/wiremock-stubs.md`'s "Model CRUD statefulness"
//!   section for the full investigation into why (vanilla WireMock
//!   scenarios hold one string per scenario, not a per-record store; a
//!   real per-record store needs the third-party
//!   `wiremock-state-extension` Java extension, a dependency choice left
//!   to the maintainer) before building anything on top of this crate
//!   that assumes otherwise.
//! - Not covered yet (tracked as follow-ups in the design doc):
//!   `transport grpc` schemas
//!   ([`WireMockGeneratorError::UnsupportedTransport`]), `FindMany<T>`
//!   return types ([`WireMockGeneratorError::UnsupportedReturnType`]),
//!   error-case stubs (WireMock scenarios/priority), request-body/query
//!   filter matching, and any emulation of the auth chokepoint every
//!   procedure/model route sits behind.
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
mod values;

pub use config::{GeneratedWireMockFile, GeneratedWireMockPackage, WireMockGeneratorConfig};
pub use error::WireMockGeneratorError;
pub use generator::generate_package;

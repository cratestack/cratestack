//! Generates WireMock stub mappings (<https://wiremock.org/docs/stubbing/>)
//! from a `.cstack` schema's procedures, so integration/e2e tests can run
//! against a mock backend whose wire contract is derived from the same
//! schema the real server is generated from, instead of a hand-maintained
//! JSON fixture that can silently drift from it. See
//! `docs/design/wiremock-stubs.md` for the motivating case, full design,
//! and open questions this crate's v1 slice deliberately leaves open.
//!
//! # Scope (v1)
//!
//! - Covered: `procedure`/`mutation procedure` declarations, under
//!   `transport rest` (the schema default) or `transport rpc`. Happy-path
//!   only — every generated stub responds `200` with a synthesized
//!   instance of the procedure's declared return type, matching on
//!   request method + path (no body assertion, no error-case variants).
//! - Not covered yet (tracked as follow-ups in the design doc): `model`
//!   blocks' REST CRUD routes, `transport grpc` schemas
//!   ([`WireMockGeneratorError::UnsupportedTransport`]), `FindMany<T>`
//!   return types ([`WireMockGeneratorError::UnsupportedReturnType`]),
//!   error-case stubs (WireMock scenarios/priority), and any emulation of
//!   the auth chokepoint every procedure sits behind.
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
mod values;

pub use config::{GeneratedWireMockFile, GeneratedWireMockPackage, WireMockGeneratorConfig};
pub use error::WireMockGeneratorError;
pub use generator::generate_package;

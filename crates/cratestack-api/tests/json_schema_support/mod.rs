//! Shared body of the JSON Schema round-trip suites (cratestack#1037, MCP
//! phase 2): `json_schema_round_trip.rs` (`decimal = RustDecimal`) and
//! `json_schema_bigdecimal.rs` (`decimal = BigDecimal`). Each suite
//! expands `tests/fixtures/json_schema_round_trip.cstack` with the real
//! `include_server_schema!` under the `mcp` feature, which yields both the
//! generated types and, in `cratestack_schema::mcp::TOOLS`, the generated
//! schemas (phase 3 replaced phase 2's hidden `__procedure_json_schemas!`
//! probe with that table, cratestack#1038). Each suite then defines the
//! backend-specific [`crate::DECIMAL`] cases and pulls in this module. The
//! tests here never construct the wire shape by hand: every positive value
//! is a generated type run through `serde_json`.
//!
//! `@computed` outputs (ADR 0002 Q7) cannot be covered here: their wire
//! value only exists after response composition, which needs a resolver
//! and a dispatch. `tests/mcp_tools.rs` round-trips one through a real
//! `tools/call` instead.
//!
//! Validation uses `jsonschema`'s 2020-12 defaults, where `format` is an
//! annotation. That is what an MCP client that doesn't opt in to format
//! assertion sees, so the generated patterns have to do the rejecting.

mod accepted_forms;
mod gaps;
mod negative;
mod round_trip;
mod values;

use jsonschema::Validator;
use serde_json::Value;

/// Per-backend `Decimal` cases, defined by each suite.
pub struct DecimalCases {
    /// Strings this backend parses, spanning every form it serializes to
    /// (plain, negative, many fractional digits, and exponent form where
    /// the backend emits it).
    pub samples: &'static [&'static str],
    /// Inputs both the schema and serde reject. These and the two lists
    /// below are JSON literals, so they can be numbers as well as strings.
    pub wrong: &'static [&'static str],
    /// Inputs serde accepts that the schema deliberately does not, because
    /// they aren't a form this backend ever emits.
    pub stricter: &'static [&'static str],
    /// Inputs the schema accepts that serde rejects: limits a pattern
    /// can't express. Pinned so a change on either side is noticed.
    pub gaps: &'static [&'static str],
}

/// One procedure's schemas, parsed and compiled.
pub struct Tool {
    pub input: Validator,
    pub output: Option<Validator>,
}

/// The tool's schemas as the generated MCP table carries them. A tool the
/// generator refused would not be in the table at all: it is a compile
/// error since phase 3 (`cratestack-macros`' `tests/ui_mcp.rs`).
pub fn tool(name: &str) -> Tool {
    let descriptor = crate::cratestack_schema::mcp::TOOLS
        .iter()
        .find(|descriptor| descriptor.name == name)
        .unwrap_or_else(|| panic!("no tool `{name}` in the fixture"));
    Tool {
        input: compile(name, descriptor.input_schema),
        output: descriptor.output_schema.map(|output| compile(name, output)),
    }
}

fn compile(name: &str, schema: &str) -> Validator {
    let schema: Value = serde_json::from_str(schema).expect("generated schema is JSON");
    jsonschema::draft202012::meta::validate(&schema)
        .unwrap_or_else(|error| panic!("`{name}` schema is not valid 2020-12: {error}"));
    jsonschema::draft202012::new(&schema)
        .unwrap_or_else(|error| panic!("`{name}` schema does not compile: {error}"))
}

pub fn assert_accepts(validator: &Validator, instance: &Value, context: &str) {
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "{context}: schema rejected serde's own output {instance}: {errors:#?}"
    );
}

pub fn assert_rejects(validator: &Validator, instance: &Value, context: &str) {
    assert!(
        !validator.is_valid(instance),
        "{context}: schema accepted wrong-shaped {instance}"
    );
}

/// `base` with `path` (dot-separated object keys) replaced by `value`, or
/// removed when `value` is `None`.
pub fn with(base: &Value, path: &str, value: Option<Value>) -> Value {
    let mut out = base.clone();
    let (parent, key) = match path.rsplit_once('.') {
        Some((parent, key)) => (
            parent.split('.').try_fold(&mut out, |v, k| v.get_mut(k)),
            key,
        ),
        None => (Some(&mut out), path),
    };
    let object = parent
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("`{path}` has no object parent"));
    match value {
        Some(value) => object.insert(key.to_owned(), value),
        None => object.remove(key),
    };
    out
}

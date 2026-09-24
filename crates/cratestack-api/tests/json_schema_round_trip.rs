//! cratestack#1037 (MCP phase 2): the generated JSON Schemas agree with
//! serde's wire shape for the real generated types, under
//! `decimal = RustDecimal`. The `decimal = BigDecimal` run is
//! `json_schema_bigdecimal.rs`; the shared cases are in
//! `json_schema_support/`, whose module doc explains the setup.

use cratestack::include_server_schema;
use json_schema_support::DecimalCases;

include_server_schema!(
    "tests/fixtures/json_schema_round_trip.cstack",
    db = None,
    decimal = RustDecimal
);

const SCHEMAS: &[(&str, Result<&str, &str>, Result<Option<&str>, &str>)] = cratestack_macros::__procedure_json_schemas!(
    "tests/fixtures/json_schema_round_trip.cstack",
    decimal = RustDecimal
);

/// `rust_decimal` (workspace feature `serde-str`), measured: always a
/// plain string, never exponent form, and a JSON number is rejected.
const DECIMAL: DecimalCases = DecimalCases {
    samples: &[
        "0",
        "-1",
        "12.50",
        "0.0000001",
        "-0.0000000005",
        "79228162514264337593543950335",
        "-79228162514264337593543950335",
        "1.2345678901234567890123456789",
    ],
    wrong: &["1.5", "2", "\"1e\""],
    stricter: &[
        "\"1e3\"",
        "\"1.5E+3\"",
        "\"+1.5\"",
        "\".5\"",
        "\"5.\"",
        "\"1_000\"",
    ],
    // Thirty digits: past `rust_decimal`'s 96-bit mantissa.
    gaps: &["\"100000000000000000000000000000\""],
};

mod json_schema_support;

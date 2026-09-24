//! cratestack#1037 (MCP phase 2): `json_schema_round_trip.rs`'s cases,
//! under `decimal = BigDecimal`. Gated `required-features =
//! ["decimal-bigdecimal", "mcp"]` in `Cargo.toml`, because the default test
//! run selects neither; `just test-ci-host` runs it explicitly (`cargo test
//! -p cratestack-api --features decimal-bigdecimal,mcp --test
//! json_schema_bigdecimal`), so it is not a silent skip.

use cratestack::include_server_schema;
use json_schema_support::DecimalCases;

include_server_schema!(
    "tests/fixtures/json_schema_round_trip.cstack",
    db = None,
    decimal = BigDecimal
);

/// `bigdecimal` (workspace feature `serde`), measured: a string, in
/// exponent form once the exponent is large (`"1E-7"`, `"1e+30"`), and a
/// JSON number is accepted on input.
const DECIMAL: DecimalCases = DecimalCases {
    samples: &[
        "0",
        "-1",
        "12.50",
        "0.0000001",
        "-5e-10",
        "1e30",
        "100000000000000000000000000000.000000000000000000001",
        "-123456789012345678901234567890123456789",
    ],
    wrong: &["\"1e\""],
    stricter: &["1.5", "2", "\"+1.5\"", "\".5\"", "\"5.\"", "\"1_000\""],
    // An exponent past `i64`: `bigdecimal` reports "Exponent overflow".
    gaps: &["\"1e9999999999999999999\""],
};

mod json_schema_support;

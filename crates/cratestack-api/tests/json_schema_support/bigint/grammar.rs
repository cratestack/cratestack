//! The grammar half of `bigint.rs`: a differential sweep over many texts, and
//! the one gap between the schema and serde.

use serde_json::json;

use super::super::tool;
use super::{assert_big_args_refused, echo_big, echo_big_args};

/// The grammar of `cratestack::BigInt`, written down here independently of
/// both the schema's pattern and `BigInt`'s parser: `0`, or an optional
/// `-`, a non-zero digit and at most 18 more digits.
fn canonical_shape(text: &str) -> bool {
    if text == "0" {
        return true;
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    let mut chars = digits.chars();
    matches!(chars.next(), Some('1'..='9'))
        && digits.len() <= 19
        && chars.all(|c| c.is_ascii_digit())
}

/// Strings built around every length from 1 to 21 digits, the `i64`
/// edges, and the decorations a producer might add.
fn candidates() -> Vec<String> {
    let mut bodies: Vec<String> = [
        "",
        "0",
        "00",
        "9223372036854775806",
        "9223372036854775807",
        "9223372036854775808",
        "9223372036854775809",
        "9007199254740993",
    ]
    .map(str::to_owned)
    .to_vec();
    for length in 1..=21 {
        bodies.push("9".repeat(length));
        bodies.push(format!("1{}", "0".repeat(length - 1)));
        bodies.push("1".repeat(length));
        bodies.push(format!("0{}", "7".repeat(length)));
    }
    let mut out = Vec::new();
    for body in &bodies {
        for prefix in ["", "-", "+", " "] {
            for suffix in ["", " ", "\n", ".0"] {
                out.push(format!("{prefix}{body}{suffix}"));
            }
        }
    }
    out.extend(
        [
            "١٢٣", "1_000", "0x10", "1e3", "Infinity", "NaN", "--1", "-+1",
        ]
        .map(str::to_owned),
    );
    out
}

/// A differential sweep: for each candidate text, the schema accepts it
/// exactly when it is in the canonical grammar, and serde accepts it
/// exactly when it is canonical *and* fits `i64`. Both predicates are
/// computed here, independently of the code under test.
#[test]
fn the_schema_and_serde_agree_with_the_grammar_over_a_sweep() {
    let tool = tool("echoBig");
    let (mut schema_accepted, mut serde_accepted) = (0, 0);
    for text in candidates() {
        let args = echo_big_args(json!(text), None, json!([]));
        let canonical = canonical_shape(&text);
        let fits = canonical && text.parse::<i64>().is_ok();
        assert_eq!(tool.input.is_valid(&args), canonical, "schema on {text:?}");
        let decoded = serde_json::from_value::<echo_big::Args>(args);
        assert_eq!(decoded.is_ok(), fits, "serde on {text:?}: {decoded:?}");
        schema_accepted += usize::from(canonical);
        serde_accepted += usize::from(fits);
    }
    assert!(
        schema_accepted > 30 && serde_accepted > 30,
        "the sweep is too weak: {schema_accepted} schema-valid, {serde_accepted} in range"
    );
}

/// The one place the schema is looser than serde: a 19-digit value past
/// `i64`. A regular expression cannot express the bound without
/// enumerating it, so the server enforces it (design section 2). Pinned
/// exactly, so a change on either side flips this test.
#[test]
fn out_of_range_values_are_the_one_documented_gap() {
    let tool = tool("echoBig");
    for text in [
        "9223372036854775808",
        "9999999999999999999",
        "-9223372036854775809",
        "-9999999999999999999",
    ] {
        let args = echo_big_args(json!(text), None, json!([]));
        assert!(
            tool.input.is_valid(&args),
            "{text}: the schema now rejects it; drop the gap"
        );
        assert!(
            serde_json::from_value::<echo_big::Args>(args).is_err(),
            "{text}: serde now accepts it; drop the gap"
        );
    }
    // Twenty digits is past the pattern, so it is not part of the gap.
    let twenty = echo_big_args(json!("10000000000000000000"), None, json!([]));
    assert_big_args_refused(&tool, twenty, "twenty digits");
}

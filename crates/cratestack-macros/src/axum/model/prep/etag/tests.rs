//! The ETag and `If-Match` tokens for a model whose `@version` column is a
//! `BigInt` (ADR 0019, PR B).
//!
//! The header is a plain decimal number whichever scalar the column is, so
//! the generated handler widens the column to `i64` with `i64::from`, which
//! accepts both an `i64` and a `cratestack::BigInt`. Before, the handler
//! wrote `Some(record.version)` into an `Option<i64>`, which cannot hold a
//! `BigInt` and failed to compile in the consumer's crate.

use super::etag_tokens;

fn tokens(version: Option<&str>) -> super::EtagTokens {
    let model = syn::Ident::new("Account", proc_macro2::Span::call_site());
    etag_tokens(&version.map(str::to_owned), &model)
}

#[test]
fn the_update_etag_widens_the_version_column_to_i64() {
    let rendered = tokens(Some("revision")).update_etag_extract.to_string();
    assert!(
        rendered.contains("Some (i64 :: from (record . revision))"),
        "{rendered}"
    );
    assert!(rendered.contains("Option < i64 >"), "{rendered}");
}

#[test]
fn the_get_etag_widens_the_version_column_to_i64() {
    let rendered = tokens(Some("revision")).get_etag_capture.to_string();
    assert_eq!(
        rendered,
        "etag_version = Some (i64 :: from (record . revision)) ;"
    );
}

#[test]
fn a_bare_version_field_read_would_not_compile_for_a_bigint() {
    // Pins the shape, not the compiler: a field read straight into the
    // `Option<i64>` is exactly what a `BigInt` column cannot satisfy.
    for rendered in [
        tokens(Some("revision")).update_etag_extract.to_string(),
        tokens(Some("revision")).get_etag_capture.to_string(),
    ] {
        assert!(
            !rendered.contains("Some (record . revision)"),
            "the version column must be widened, not assigned: {rendered}"
        );
    }
}

#[test]
fn if_match_stays_an_i64_header_value() {
    let tokens = tokens(Some("revision"));
    assert!(
        tokens
            .update_if_match_decl
            .to_string()
            .contains("parse_if_match_version")
    );
    assert_eq!(
        tokens.update_if_match_apply.to_string(),
        ". if_match (if_match_version . unwrap ())"
    );
    assert_eq!(
        tokens.delete_if_match_apply.to_string(),
        ". if_match (if_match_version . unwrap ())"
    );
}

#[test]
fn a_model_without_a_version_column_gets_no_etag_tokens() {
    let tokens = tokens(None);
    assert!(tokens.update_etag_extract.is_empty());
    assert!(tokens.get_etag_capture.is_empty());
    assert!(tokens.update_if_match_apply.is_empty());
}

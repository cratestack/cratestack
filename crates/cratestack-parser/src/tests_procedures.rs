#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_isolation_attribute_on_procedure() {
    let schema = parse_schema(
        r#"
type TransferInput {
  from Int
  to Int
}

mutation procedure transfer(args: TransferInput): TransferInput
  @isolation("serializable")
"#,
    )
    .expect("procedure with @isolation should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs
            .iter()
            .any(|a| a.raw == "@isolation(\"serializable\")"),
        "expected @isolation in attributes: {attrs:?}",
    );
}

#[test]
fn accepts_isolation_repeatable_read() {
    parse_schema(
        r#"
type Ping {
  nonce String
}

procedure read_only(args: Ping): Ping
  @isolation("repeatable_read")
"#,
    )
    .expect("repeatable_read isolation should parse");
}

#[test]
fn rejects_invalid_isolation_level() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @isolation("snapshot")
"#,
    )
    .expect_err("unknown isolation level should fail");

    assert!(
        error
            .to_string()
            .contains("unknown transaction isolation level"),
        "error: {error}",
    );
}

#[test]
fn rejects_isolation_missing_argument() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @isolation
"#,
    )
    .expect_err("@isolation without args should fail");

    assert!(
        error
            .to_string()
            .contains("@isolation requires a quoted level argument"),
        "error: {error}",
    );
}

#[test]
fn accepts_api_version_and_deprecated_on_procedure() {
    let schema = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure healthcheck(args: Ping): Ping
  @api_version("v1")
  @deprecated("use healthcheck_v2")
"#,
    )
    .expect("procedure with @api_version + @deprecated should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs.iter().any(|a| a.raw == "@api_version(\"v1\")"),
        "expected @api_version: {attrs:?}",
    );
    assert!(
        attrs
            .iter()
            .any(|a| a.raw == "@deprecated(\"use healthcheck_v2\")"),
        "expected @deprecated",
    );
}

#[test]
fn rejects_empty_api_version() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure healthcheck(args: Ping): Ping
  @api_version("")
"#,
    )
    .expect_err("empty @api_version should fail");

    assert!(
        error.to_string().contains("@api_version must not be empty"),
        "error: {error}",
    );
}

#[test]
fn rejects_api_version_with_invalid_characters() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure healthcheck(args: Ping): Ping
  @api_version("v 1")
"#,
    )
    .expect_err("@api_version with space should fail");

    assert!(
        error.to_string().contains("must contain only alphanumeric"),
        "error: {error}",
    );
}

#[test]
fn parses_no_idempotency_attribute_on_procedure() {
    let schema = parse_schema(
        r#"
type Ping {
  nonce String
}

mutation procedure healthcheck(args: Ping): Ping
  @no_idempotency
"#,
    )
    .expect("procedure with @no_idempotency should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs.iter().any(|a| a.raw == "@no_idempotency"),
        "procedure attributes should include @no_idempotency: {:?}",
        attrs,
    );
}

/// cratestack#154: `@no_rate_limit` is valid syntax when the schema
/// declares `extension rate_limit { }`.
#[test]
fn accepts_no_rate_limit_attribute_when_extension_is_declared() {
    let schema = parse_schema(
        r#"
extension rate_limit {
}

type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
  @no_rate_limit
"#,
    )
    .expect("@no_rate_limit should parse when `extension rate_limit { }` is declared");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs.iter().any(|a| a.raw == "@no_rate_limit"),
        "procedure attributes should include @no_rate_limit: {:?}",
        attrs,
    );
}

/// cratestack#154: using `@no_rate_limit` without declaring `extension
/// rate_limit { }` anywhere in the schema is a validation error, not a
/// silent no-op — mirrors the Cargo-feature-gate error #161 raises at macro
/// expansion time, but this one fires at parse/validate time since the
/// parser already knows whether the extension is declared.
#[test]
fn rejects_no_rate_limit_attribute_without_extension_declared() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
  @no_rate_limit
"#,
    )
    .expect_err("@no_rate_limit without `extension rate_limit { }` should be a validation error");

    let message = error.to_string();
    assert!(
        message.contains("@no_rate_limit"),
        "error should name the offending attribute, got: {message}",
    );
    assert!(
        message.contains("extension rate_limit"),
        "error should point at the missing `extension rate_limit {{ }}` declaration, got: {message}",
    );
}

/// cratestack#154: `@no_rate_limit` takes no arguments.
#[test]
fn rejects_no_rate_limit_attribute_with_arguments() {
    let error = parse_schema(
        r#"
extension rate_limit {
}

type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
  @no_rate_limit("forever")
"#,
    )
    .expect_err("@no_rate_limit should not accept any arguments");

    assert!(
        error.to_string().contains("does not take any arguments"),
        "error: {error}",
    );
}

/// cratestack#407: `@status(202)` is accepted and threaded into
/// `procedure.attributes` verbatim, mirroring `@api_version`/`@deprecated`.
#[test]
fn accepts_status_attribute_on_procedure() {
    let schema = parse_schema(
        r#"
type Ping {
  nonce String
}

mutation procedure submit(args: Ping): Ping
  @status(202)
"#,
    )
    .expect("procedure with @status(202) should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs.iter().any(|a| a.raw == "@status(202)"),
        "expected @status(202) in attributes: {attrs:?}",
    );
}

/// cratestack#407: the boundary values of the allowed `200..=299` range are
/// both accepted.
#[test]
fn accepts_status_boundary_values() {
    parse_schema(
        r#"
type Ping {
  nonce String
}

procedure lower(args: Ping): Ping
  @status(200)
"#,
    )
    .expect("@status(200) should parse");

    parse_schema(
        r#"
type Ping {
  nonce String
}

procedure upper(args: Ping): Ping
  @status(299)
"#,
    )
    .expect("@status(299) should parse");
}

/// cratestack#407: anything outside `200..=299` is a schema-compile-time
/// error, not a runtime surprise — `CratestackError` already owns the
/// 3xx/4xx/5xx space.
#[test]
fn rejects_status_outside_2xx_range() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @status(404)
"#,
    )
    .expect_err("@status(404) should be rejected");

    assert!(
        error.to_string().contains("outside the allowed 2xx range"),
        "error: {error}",
    );
}

/// cratestack#407: `@status(300)` (just above the allowed range) is
/// rejected too — this pins the exact boundary as a regression guard.
#[test]
fn rejects_status_just_above_range() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @status(300)
"#,
    )
    .expect_err("@status(300) should be rejected");

    assert!(
        error.to_string().contains("outside the allowed 2xx range"),
        "error: {error}",
    );
}

/// cratestack#407: `@status` requires a numeric argument.
#[test]
fn rejects_status_non_numeric_argument() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @status("202")
"#,
    )
    .expect_err("@status with a quoted argument should be rejected");

    assert!(
        error.to_string().contains("integer HTTP status code"),
        "error: {error}",
    );
}

/// cratestack#407: `@status` without an argument is rejected.
#[test]
fn rejects_status_missing_argument() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @status
"#,
    )
    .expect_err("@status without args should fail");

    assert!(
        error
            .to_string()
            .contains("@status requires a numeric status code argument"),
        "error: {error}",
    );
}

/// cratestack#407: at most one `@status` per procedure.
#[test]
fn rejects_duplicate_status_attribute() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure broken(args: Ping): Ping
  @status(202)
  @status(201)
"#,
    )
    .expect_err("more than one @status on the same procedure should be rejected");

    assert!(
        error.to_string().contains("more than one @status"),
        "error: {error}",
    );
}

/// cratestack#407 follow-up: `@status` is a REST-only attribute.
/// `generate_procedure_axum_handler` emits a single `#dispatch_ident`
/// shared by both the REST route and the `transport rpc` unary dispatch
/// arm, so an unrejected `@status` here would silently become
/// wire-visible on the RPC envelope too (`convert_handler_error_response`
/// passes any `is_success()` status through unchanged). Reject the
/// combination at schema-compile time instead.
#[test]
fn rejects_status_attribute_under_transport_rpc() {
    let error = parse_schema(
        r#"
transport rpc

type Ping {
  nonce String
}

mutation procedure submit(args: Ping): Ping
  @status(202)
"#,
    )
    .expect_err("@status under `transport rpc` should be rejected");

    assert!(
        error.to_string().contains("REST-only attribute"),
        "error: {error}",
    );
}

/// cratestack#407 follow-up: `transport grpc` is unaffected by the
/// `transport rpc` rejection above — gRPC's own status model never
/// reads the HTTP status `@status` controls, so the combination is
/// inert there, not silently wrong, and stays accepted.
#[test]
fn accepts_status_attribute_under_transport_grpc() {
    parse_schema(
        r#"
transport grpc

type Ping {
  nonce String
}

mutation procedure submit(args: Ping): Ping
  @status(202)
"#,
    )
    .expect("@status under `transport grpc` should still parse");
}

/// cratestack#154: at most one `@no_rate_limit` per procedure.
#[test]
fn rejects_duplicate_no_rate_limit_attribute() {
    let error = parse_schema(
        r#"
extension rate_limit {
}

type Ping {
  nonce String
}

mutation procedure createPayment(args: Ping): Ping
  @no_rate_limit
  @no_rate_limit
"#,
    )
    .expect_err("more than one @no_rate_limit on the same procedure should be rejected");

    assert!(
        error.to_string().contains("more than one @no_rate_limit"),
        "error: {error}",
    );
}

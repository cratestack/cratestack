//! A valid schema (the shape of fespalier's `examples/cratestack`
//! `server/schema.cstack` @ aa47d0c, whose six procedures carry the
//! marker) and helpers to break it one way at a time.

use cratestack_core::Schema;

use crate::{SchemaError, parse_schema};

pub(super) const BASE: &str = r#"transport rpc

datasource db {
  provider = "none"
}

type Jwk {
  kty String
  crv String
  x String
  y String
}

type SendCodeArgs {
  phone String
}

type CodeSent {
  phone String
}

type BeginEnrolmentArgs {
  phone String
  otp String
  ambient Jwk
  userPresent Jwk?
}

type EnrolmentChallenges {
  enrolmentId String
  ambient String
  userPresent String?
}

type ConfirmEnrolmentArgs {
  enrolmentId String
  ambient String
  userPresent String?
}

type EnrolledDevice {
  deviceId String
  userId String
  phone String
  ambient String
  userPresent String?
}

type BeginRenewalArgs {
  deviceId String
  role String
}

type RenewalChallenge {
  challengeId String
  challenge String
}

type ConfirmRenewalArgs {
  challengeId String
  signature String
}

type Renewed {
  attestation String
}

type RevokeArgs {
  deviceId String
}

type Revoked {
  deviceId String
}

mutation procedure sendEnrolmentCode(args: SendCodeArgs): CodeSent
  @device_auth(role: "sendEnrolmentCode")
  @allow(true)

mutation procedure beginEnrolment(args: BeginEnrolmentArgs): EnrolmentChallenges
  @device_auth(role: "beginEnrolment")
  @allow(true)

mutation procedure confirmEnrolment(args: ConfirmEnrolmentArgs): EnrolledDevice
  @device_auth(role: "confirmEnrolment")
  @allow(true)

mutation procedure beginRenewal(args: BeginRenewalArgs): RenewalChallenge
  @device_auth(role: "beginRenewal")
  @allow(true)

mutation procedure confirmRenewal(args: ConfirmRenewalArgs): Renewed
  @device_auth(role: "confirmRenewal")
  @allow(true)

mutation procedure revokeDevice(args: RevokeArgs): Revoked
  @device_auth(role: "revoke")
  @allow(auth() != null)
"#;

pub(super) fn parse(source: &str) -> Result<Schema, SchemaError> {
    parse_schema(source)
}

/// `BASE` with the one occurrence of `from` replaced by `to`.
pub(super) fn edit(from: &str, to: &str) -> String {
    assert_eq!(
        BASE.matches(from).count(),
        1,
        "`{from}` must be unique in BASE"
    );
    BASE.replace(from, to)
}

/// The error `source` fails with, which must contain `reason`.
pub(super) fn refused(source: &str, reason: &str) -> SchemaError {
    let error = parse(source).expect_err(reason);
    assert!(
        error.message().contains(reason),
        "expected `{reason}` in: {}",
        error.message()
    );
    error
}

/// The 1-based line of `needle`'s first occurrence in `source`.
pub(super) fn line_of(source: &str, needle: &str) -> usize {
    let at = source.find(needle).expect("needle in source");
    source[..at].matches('\n').count() + 1
}

/// Replaces `from` in `BASE`, expects `reason`, and the error at the marker `role`.
pub(super) fn broken(role: &str, from: &str, to: &str, reason: &str) {
    let source = edit(from, to);
    let error = refused(&source, reason);
    let marker = format!("@device_auth(role: \"{role}\")");
    assert_eq!(error.line(), line_of(&source, &marker), "{reason}");
    assert!(
        error.message().contains(&format!("(`{marker}`)")),
        "names the role: {}",
        error.message()
    );
}

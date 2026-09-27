//! Positive controls for `tests_procedure_policy_spelling`
//! (GHSA-69g4-xvcm-vm2j): every legitimate procedure attribute spelling the
//! repo's schemas and docs use still parses under the closed allowlist, and
//! `@` and `//` inside a string literal are left alone.

use crate::parse_schema;
use crate::tests_procedure_policy_spelling::HEAD;

#[test]
fn every_legitimate_procedure_attribute_still_parses() {
    let source = format!(
        "{HEAD}extension rate_limit {{\n}}\n\n\
         mutation procedure transfer(args: TransferInput): Receipt\n  \
         @allow(auth() != null && auth().role == \"teller\")\n  \
         @deny(hasRole(\"banned\") || auth().id == args.accountId)\n  \
         @authorize(Account, update, args.accountId)\n  @api_version(\"v2\")\n  \
         @status(202)\n  @deprecated(\"use transfer2\")\n  @no_idempotency\n  \
         @no_rate_limit\n  @isolation(\"serializable\")\n  @allow(auth().note == \"a@b // c\")\n\n\
         procedure feed(args: TransferInput): Many[]\n  @allow(true)\n  @stream\n  @deprecated\n"
    );
    let schema = parse_schema(&source).unwrap_or_else(|error| panic!("should parse: {error}"));
    assert_eq!(schema.procedures[0].attributes.len(), 10);
    assert_eq!(
        schema.procedures[0].attributes[9].raw,
        "@allow(auth().note == \"a@b // c\")"
    );
    assert_eq!(schema.procedures[1].attributes.len(), 3);
}

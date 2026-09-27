//! Procedure attributes the generator did not read (GHSA-69g4-xvcm-vm2j,
//! maintainer decision 2). Every row of `REFUSED` passed `cratestack check`
//! before this fix while `cratestack-macros` silently dropped the rule, so
//! the procedure admitted callers its written `@deny` refuses (or skipped
//! its `@authorize` database check). Each is now a parse error.

use crate::parse_schema;

pub(crate) const HEAD: &str = "auth SessionUser {\n  id Int\n  role String\n}\n\n\
                    model Account {\n  id Int @id\n  owner Int\n  \
                    @@allow(\"all\", auth() != null)\n}\n\n\
                    type TransferInput {\n  accountId Int\n  amount Int\n}\n\n\
                    type Receipt {\n  ok Boolean\n}\n\n\
                    type Many {\n  n Int\n}\n\n";

/// `transfer` with `@allow(auth() != null)` and then `line`.
fn transfer(line: &str) -> String {
    format!(
        "{HEAD}mutation procedure transfer(args: TransferInput): Receipt\n  \
         @allow(auth() != null)\n  {line}\n"
    )
}

/// `(attribute line, text the error must contain)`.
const REFUSED: &[(&str, &str)] = &[
    (
        "@deny (hasRole(\"banned\"))",
        "`@deny` must be followed directly by its `(`",
    ),
    (
        "@deny\t(hasRole(\"banned\"))",
        "`@deny` must be followed directly by its `(`",
    ),
    (
        "@deny\u{a0}(hasRole(\"banned\"))",
        "`@deny` must be followed directly by its `(`",
    ),
    (
        "@Deny(hasRole(\"banned\"))",
        "unsupported attribute `@Deny` on a procedure (did you mean `@deny`?)",
    ),
    (
        "@DENY(hasRole(\"banned\"))",
        "unsupported attribute `@DENY` on a procedure (did you mean `@deny`?)",
    ),
    (
        "@deyn(hasRole(\"banned\"))",
        "unsupported attribute `@deyn` on a procedure (did you mean `@deny`?)",
    ),
    (
        "@ deny(hasRole(\"banned\"))",
        "no space is allowed between `@` and the attribute name",
    ),
    (
        "@deny(hasRole(\"banned\"));",
        "`;` after the closing `)` of `@deny`",
    ),
    (
        "@deny(hasRole(\"banned\")),",
        "`,` after the closing `)` of `@deny`",
    ),
    (
        "@deny(hasRole(\"banned\")) banned",
        "`banned` after the closing `)` of `@deny`",
    ),
    (
        "@deny（hasRole(\"banned\"))",
        "after `@deny` is not part of any attribute",
    ),
    ("@deny", "`@deny` takes an argument list"),
    ("@deny()", "`@deny()` has an empty argument list"),
    (
        "@deny(hasRole(\"banned\"))@allow(true)",
        "attributes with no space between them",
    ),
    (
        "@audit @deny(hasRole(\"banned\"))",
        "unsupported attribute `@audit` on a procedure",
    ),
    ("@Allow(auth() != null)", "(did you mean `@allow`?)"),
    (
        "@allow (auth() != null)",
        "`@allow` must be followed directly by its `(`",
    ),
    (
        "@authorize (Account, update, args.accountId)",
        "`@authorize` must be followed directly",
    ),
    (
        "@Authorize(Account, update, args.accountId)",
        "(did you mean `@authorize`?)",
    ),
    (
        "@authorise(Account, update, args.accountId)",
        "(did you mean `@authorize`?)",
    ),
    (
        "@authorize(Account, update, args.accountId);",
        "`;` after the closing `)` of `@authorize`",
    ),
    (
        "@authorize(Account, update)",
        "`@authorize` takes exactly three arguments",
    ),
    (
        "@authorize(Account, updte, args.accountId)",
        "supports the actions detail, read, update, delete, not `updte`",
    ),
    ("@stream()", "`@stream` does not take arguments"),
    (
        "@no_idempotency ()",
        "after `@no_idempotency` is not part of any attribute",
    ),
    (
        "@no_idempotency,",
        "after `@no_idempotency` is not part of any attribute",
    ),
    (
        "@deprecated (\"old\")",
        "`@deprecated` must be followed directly by its `(`",
    ),
];

#[test]
fn every_spelling_the_generator_skipped_is_refused() {
    for (line, needle) in REFUSED {
        let source = transfer(line);
        let error = parse_schema(&source)
            .err()
            .unwrap_or_else(|| panic!("`{line}` must be refused, but the schema parsed"));
        let message = error.to_string();
        assert!(message.contains(needle), "`{line}`: {message}");
        assert!(
            message.contains("procedure `transfer`"),
            "`{line}`: {message}"
        );
    }
}

#[test]
fn a_policy_after_another_attribute_on_one_line_is_read() {
    for (line, expected) in [
        (
            "@no_idempotency @deny(hasRole(\"banned\"))",
            &["@no_idempotency", "@deny(hasRole(\"banned\"))"][..],
        ),
        (
            "@deprecated(\"old\")  @deny(hasRole(\"banned\"))",
            &["@deprecated(\"old\")", "@deny(hasRole(\"banned\"))"],
        ),
        (
            "@deny(hasRole(\"banned\")) @deny(hasRole(\"frozen\"))",
            &["@deny(hasRole(\"banned\"))", "@deny(hasRole(\"frozen\"))"],
        ),
        (
            "@authorize(Account, update, args.accountId) @no_idempotency",
            &[
                "@authorize(Account, update, args.accountId)",
                "@no_idempotency",
            ],
        ),
        (
            "@deny(hasRole(\"banned\")) // banned users may not move money",
            &["@deny(hasRole(\"banned\"))"],
        ),
    ] {
        let source = transfer(line);
        let schema =
            parse_schema(&source).unwrap_or_else(|error| panic!("`{line}` should parse: {error}"));
        let procedure = &schema.procedures[0];
        let raws = procedure.attributes[1..]
            .iter()
            .map(|a| a.raw.as_str())
            .collect::<Vec<_>>();
        assert_eq!(raws, expected, "{line}");
        for attribute in &procedure.attributes {
            let (start, end) = (attribute.span.start, attribute.span.end);
            assert_eq!(
                &source[start..end],
                attribute.raw,
                "span of `{}`",
                attribute.raw
            );
        }
    }
}

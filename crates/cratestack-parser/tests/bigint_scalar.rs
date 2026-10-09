//! `BigInt` as a built-in scalar (ADR 0019, PR B): wherever `Int` is
//! recognised, `BigInt` is too.
//!
//! In the parser IR a scalar is a [`TypeRef`] whose `name` is the schema
//! spelling, so `BigInt` is `TypeRef { name: "BigInt", .. }` and there is no
//! enum variant to match on. Every position below is asserted by reading that
//! name back out of the parsed [`Schema`], not just by "it parsed".

use cratestack_core::{Schema, TypeArity, TypeRef};
use cratestack_parser::{builtin_type_names, parse_schema};

const I64_MAX: &str = "9223372036854775807";
const I64_MIN: &str = "-9223372036854775808";
const ABOVE_I64_MAX: &str = "9223372036854775808";
const BELOW_I64_MIN: &str = "-9223372036854775809";

fn parsed(source: &str) -> Schema {
    parse_schema(source).unwrap_or_else(|error| panic!("expected the schema to parse: {error}"))
}

/// The first error `source` produces. A schema that parses when a rejection
/// was expected panics here, so a refusal test cannot pass by accident.
fn refused(source: &str) -> String {
    match parse_schema(source) {
        Ok(_) => panic!("expected the schema to be refused, but it parsed:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn model_with(field: &str) -> String {
    format!("model Account {{\n  id Int @id\n  {field}\n}}\n")
}

fn assert_bigint(ty: &TypeRef, arity: TypeArity) {
    assert_eq!(ty.name, "BigInt");
    assert_eq!(ty.arity, arity);
    assert!(ty.generic_args.is_empty());
    assert!(ty.int_args.is_empty());
    assert!(ty.ident_args.is_empty());
}

#[test]
fn bigint_is_a_builtin_type_name() {
    let names = builtin_type_names();
    assert!(names.contains(&"BigInt"), "{names:?}");
    assert!(
        names.contains(&"Int"),
        "Int must stay a built-in: {names:?}"
    );
}

#[test]
fn bigint_is_a_plain_scalar_type_ref() {
    let schema = parsed(&model_with("balance BigInt"));
    assert_bigint(&schema.models[0].fields[1].ty, TypeArity::Required);
}

#[test]
fn bigint_keeps_its_arity_in_optional_position() {
    let schema = parsed(&model_with("balance BigInt?"));
    assert_bigint(&schema.models[0].fields[1].ty, TypeArity::Optional);
}

#[test]
fn bigint_is_valid_as_a_primary_key_and_a_foreign_key() {
    let schema = parsed(
        r#"
model Account {
  id BigInt @id
}

model Entry {
  id BigInt @id
  accountId BigInt
  account Account @relation(fields: [accountId], references: [id])
}
"#,
    );
    let account = &schema.models[0];
    let entry = &schema.models[1];
    assert_bigint(&account.fields[0].ty, TypeArity::Required);
    assert!(account.fields[0].is_primary_key());
    assert_bigint(&entry.fields[1].ty, TypeArity::Required);
}

#[test]
fn bigint_is_valid_on_every_declaration_kind() {
    let schema = parsed(
        r#"
datasource db {
  provider = "postgresql"
}

auth Principal {
  id BigInt
  tenant BigInt
}

mixin Stamped {
  stamp BigInt
}

type Money {
  cents BigInt
}

model Account {
  @use(Stamped)
  id Int @id
  balance BigInt
}

view AccountBalance from Account {
  id Int @id @from(Account.id)
  balance BigInt @from(Account.balance)

  @@server_sql("SELECT id, balance FROM account")
  @@embedded_sql("SELECT id, balance FROM account")
}
"#,
    );

    let auth = schema.auth.as_ref().expect("auth block");
    assert_bigint(&auth.fields[0].ty, TypeArity::Required);
    assert_bigint(&auth.fields[1].ty, TypeArity::Required);
    assert_bigint(&schema.mixins[0].fields[0].ty, TypeArity::Required);
    assert_bigint(&schema.types[0].fields[0].ty, TypeArity::Required);
    let account = &schema.models[0];
    assert!(
        account
            .fields
            .iter()
            .any(|f| f.name == "stamp" && f.ty.name == "BigInt"),
        "mixin field expanded onto the model: {:?}",
        account.fields.iter().map(|f| &f.name).collect::<Vec<_>>(),
    );
    assert_bigint(&account.fields[2].ty, TypeArity::Required);
    assert_bigint(&schema.views[0].fields[1].ty, TypeArity::Required);
}

#[test]
fn bigint_is_valid_in_procedure_arguments_and_returns() {
    let schema = parsed(
        r#"
procedure echo(n: BigInt): BigInt
procedure maybe(n: BigInt?): BigInt?
procedure many(ns: BigInt[]): BigInt[]
"#,
    );
    let [echo, maybe, many] = schema.procedures.as_slice() else {
        panic!("three procedures expected");
    };
    assert_bigint(&echo.args[0].ty, TypeArity::Required);
    assert_bigint(&echo.return_type, TypeArity::Required);
    assert_bigint(&maybe.args[0].ty, TypeArity::Optional);
    assert_bigint(&maybe.return_type, TypeArity::Optional);
    assert_bigint(&many.args[0].ty, TypeArity::List);
    assert_bigint(&many.return_type, TypeArity::List);
}

#[test]
fn bigint_is_valid_in_a_type_used_as_a_procedure_argument_and_return() {
    let schema = parsed(
        r#"
type Transfer {
  amount BigInt
  note String
}

mutation procedure move(args: Transfer): Transfer
"#,
    );
    assert_bigint(&schema.types[0].fields[0].ty, TypeArity::Required);
}

#[test]
fn bigint_is_a_bindable_query_parameter_and_a_result_field() {
    let schema = parsed(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

type Totals {
  total BigInt
}

query totalFor(accountId: BigInt, floor: Int): Totals
  @@sql("SELECT SUM(amount)::bigint AS total FROM entry WHERE account_id = $1 AND amount >= $2")
  @allow(true)
"#,
    );
    let query = &schema.queries[0];
    assert_bigint(&query.args[0].ty, TypeArity::Required);
    assert_eq!(query.args[1].ty.name, "Int");
    assert_bigint(&schema.types[0].fields[0].ty, TypeArity::Required);
}

#[test]
fn an_unbindable_query_parameter_message_lists_bigint_as_supported() {
    let message = refused(
        r#"
type Totals {
  total BigInt
}

query totalFor(amount: Decimal): Totals
  @@sql("SELECT 1::bigint AS total")
  @allow(true)
"#,
    );
    assert!(message.contains("`Decimal`"), "{message}");
    assert!(
        message.contains("Int, BigInt, Float"),
        "BigInt belongs in the supported list: {message}",
    );
}

#[test]
fn other_spellings_of_bigint_are_unknown_types() {
    for spelling in ["Bigint", "bigint", "BIGINT", "Int64", "Long"] {
        let message = refused(&model_with(&format!("balance {spelling}")));
        assert!(
            message.contains(&format!("unknown type `{spelling}`")),
            "{spelling}: {message}",
        );
    }
}

#[test]
fn a_user_declaration_named_bigint_is_a_duplicate_name() {
    let cases = [
        (
            "type BigInt {\n  v String\n}\n",
            "duplicate type name `BigInt`",
        ),
        ("enum BigInt {\n  A\n}\n", "duplicate enum name `BigInt`"),
        (
            "model BigInt {\n  id Int @id\n}\n",
            "duplicate model name `BigInt`",
        ),
        (
            "mixin BigInt {\n  v String\n}\n",
            "duplicate mixin name `BigInt`",
        ),
        (
            "auth BigInt {\n  id Int\n}\n",
            "duplicate auth type name `BigInt`",
        ),
    ];
    for (source, expected) in cases {
        let message = refused(source);
        assert!(message.contains(expected), "{expected}: {message}");
    }
}

// ---- @version ----------------------------------------------------------

#[test]
fn version_is_accepted_on_a_required_bigint() {
    let schema = parsed(&model_with("version BigInt @version"));
    let field = &schema.models[0].fields[1];
    assert_bigint(&field.ty, TypeArity::Required);
    assert!(field.attributes.iter().any(|a| a.raw == "@version"));
}

#[test]
fn version_is_still_accepted_on_a_required_int() {
    parsed(&model_with("version Int @version"));
}

#[test]
fn version_is_refused_on_an_optional_or_list_bigint() {
    for ty in ["BigInt?", "BigInt[]"] {
        let message = refused(&model_with(&format!("version {ty} @version")));
        assert!(
            message.contains("must be a required `Int` or `BigInt`"),
            "{ty}: {message}",
        );
        assert!(message.contains("Account.version"), "{ty}: {message}");
    }
}

#[test]
fn version_is_still_refused_on_other_scalars_and_on_the_primary_key() {
    let message = refused(&model_with("version Decimal @version"));
    assert!(
        message.contains("must be a required `Int` or `BigInt`"),
        "{message}",
    );

    let message = refused("model Account {\n  id BigInt @id @version\n}\n");
    assert!(
        message.contains("must not also be the primary key"),
        "{message}"
    );
}

// ---- @range ------------------------------------------------------------

#[test]
fn range_is_accepted_on_bigint_with_i64_bounds() {
    for args in [
        format!("min: {I64_MIN}, max: {I64_MAX}"),
        "min: 0, max: 1000000000000".to_owned(),
        format!("min: {I64_MIN}"),
        format!("max: {I64_MAX}"),
        "min: -5, max: -5".to_owned(),
    ] {
        let schema = parsed(&model_with(&format!("amount BigInt @range({args})")));
        let field = &schema.models[0].fields[1];
        assert_bigint(&field.ty, TypeArity::Required);
        assert!(
            field
                .attributes
                .iter()
                .any(|a| a.raw == format!("@range({args})")),
            "{args}: {:?}",
            field.attributes,
        );
    }
}

#[test]
fn range_on_bigint_refuses_a_bound_outside_i64() {
    for bound in [ABOVE_I64_MAX, BELOW_I64_MIN, "99999999999999999999"] {
        for key in ["min", "max"] {
            let message = refused(&model_with(&format!(
                "amount BigInt @range({key}: {bound})"
            )));
            assert!(
                message.contains("@range expects integer") && message.contains(bound),
                "{key}: {bound}: {message}",
            );
        }
    }
}

#[test]
fn range_on_bigint_refuses_a_non_integer_bound_and_min_above_max() {
    let message = refused(&model_with("amount BigInt @range(min: 1.5)"));
    assert!(message.contains("@range expects integer"), "{message}");

    let message = refused(&model_with("amount BigInt @range(min: 10, max: 1)"));
    assert!(message.contains("min (10) must be <= max (1)"), "{message}");

    let message = refused(&model_with("amount BigInt @range"));
    assert!(message.contains("@range requires arguments"), "{message}");
}

#[test]
fn range_is_still_refused_off_numeric_fields_and_names_bigint() {
    let message = refused(&model_with("name String @range(min: 0)"));
    assert!(
        message.contains("only valid on Int, BigInt or Decimal"),
        "{message}",
    );
}

#[test]
fn string_only_validators_are_still_refused_on_bigint() {
    let message = refused(&model_with("amount BigInt @length(min: 1)"));
    assert!(
        message.contains("only valid on String or Bytes"),
        "{message}"
    );
    let message = refused(&model_with("amount BigInt @email"));
    assert!(message.contains("only valid on String"), "{message}");
}

// ---- @default ----------------------------------------------------------

#[test]
fn default_integer_literals_are_accepted_on_bigint() {
    for literal in ["0", "-1", "42", "+7", I64_MAX, I64_MIN] {
        let schema = parsed(&model_with(&format!("amount BigInt @default({literal})")));
        let field = &schema.models[0].fields[1];
        assert_bigint(&field.ty, TypeArity::Required);
        assert!(
            field
                .attributes
                .iter()
                .any(|a| a.raw == format!("@default({literal})")),
            "{literal}: {:?}",
            field.attributes,
        );
    }
}

#[test]
fn default_functions_and_markers_are_left_alone_on_bigint() {
    for default in ["autoincrement()", "dbgenerated()", "now()"] {
        parsed(&model_with(&format!("seq BigInt @default({default})")));
    }
    parsed(
        r#"
auth Principal {
  id BigInt
}

model Account {
  id Int @id
  owner BigInt @default(auth().id)
}
"#,
    );
}

#[test]
fn default_literal_outside_i64_is_refused_on_bigint() {
    for literal in [
        ABOVE_I64_MAX,
        BELOW_I64_MIN,
        "99999999999999999999",
        "+9223372036854775808",
    ] {
        let message = refused(&model_with(&format!("amount BigInt @default({literal})")));
        assert!(
            message.contains(&format!("@default({literal})"))
                && message.contains("Account.amount")
                && message.contains("`BigInt` range"),
            "{literal}: {message}",
        );
        assert!(
            message.contains(I64_MIN) && message.contains(I64_MAX),
            "{message}"
        );
    }
}

#[test]
fn default_that_is_not_a_decimal_integer_is_refused_on_bigint() {
    for literal in ["1.5", "1e3", "0x10", "1_000", "12abc"] {
        let message = refused(&model_with(&format!("amount BigInt @default({literal})")));
        assert!(
            message.contains(&format!("@default({literal})")),
            "{literal}: {message}",
        );
    }
}

#[test]
fn default_check_reaches_a_bigint_field_that_came_from_a_mixin() {
    let message = refused(&format!(
        r#"
mixin Counted {{
  hits BigInt @default({ABOVE_I64_MAX})
}}

model Counter {{
  @use(Counted)
  id Int @id
}}
"#
    ));
    assert!(message.contains("`BigInt` range"), "{message}");
}

#[test]
fn default_check_is_scoped_to_bigint() {
    // `Int` is still `i64` in this release and its defaults are unchecked, as
    // before; other scalars keep their own spellings.
    parsed(&model_with("flag Boolean @default(true)"));
    parsed(&model_with("label String @default(\"n/a\")"));
    parsed(&model_with("count Int @default(5)"));
}

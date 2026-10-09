//! Field attributes written in a form no generator reads
//! (`validate::attribute_shape::check_shape`, the check every closed list
//! shares): an argument list or stray punctuation on an attribute that takes
//! no arguments, and two attributes with no space between them.

use super::parse_schema;

#[track_caller]
fn refused(source: &str, needle: &str) {
    let error = parse_schema(source).expect_err("schema should be refused");
    assert!(error.to_string().contains(needle), "error: {error}");
}

fn model_with(attributes: &str) -> String {
    format!("model User {{\n  id Int @id\n  value String {attributes}\n}}\n")
}

// Every no-argument name of the model list, with an argument list, on a model
// field. `@email`/`@uri`/`@iso4217` are refused there by `validators` first.
#[test]
fn refuses_an_argument_list_on_each_no_argument_attribute() {
    for name in [
        "@server_only",
        "@readonly",
        "@pii",
        "@sensitive",
        "@db_enforce",
        "@unique",
    ] {
        refused(
            &model_with(&format!("{name}()")),
            &format!(
                "writes `{name}()`: `{name}` does not take arguments, and a generator \
                 recognises it only when written exactly `{name}`"
            ),
        );
    }
    refused(
        "model User {\n  id Int @id\n  rev Int @version(1)\n}\n",
        "writes `@version(1)`: `@version` does not take arguments",
    );
    refused(
        "model User {\n  id Int @id(map: \"pk\")\n}\n",
        "writes `@id(map: \"pk\")`: `@id` does not take arguments",
    );
    for name in ["@email", "@uri", "@iso4217"] {
        refused(&model_with(&format!("{name}()")), "does not take arguments");
        // `validators` refuses the argument list on a model field too, so a
        // mixin field that no model uses is what shows the closed list covers each of
        // the three.
        refused(
            &format!("mixin M {{\n  s String {name}()\n}}\nmodel A {{\n  id Int @id\n}}\n"),
            &format!("writes `{name}()`: `{name}` does not take arguments"),
        );
    }
}

// The same rule on the other blocks, which `validators` never sees. A `type`
// field has no case here: its attributes are the validator family, which
// `validators` checks first, as on a model (`tests_type_field_validators`);
// the auth block takes no attribute at all (`tests_field_attribute_lists`).
#[test]
fn refuses_the_wrong_argument_shape_in_every_field_bearing_block() {
    for (source, needle) in [
        (
            "mixin M {\n  s String @readonly()\n}\nmodel A {\n  id Int @id\n}\n",
            "field `s` on mixin `M` writes `@readonly()`",
        ),
        (
            "model A {\n  id Int @id\n}\n\
             view V from A {\n  id Int @id() @from(A.id)\n  @@sql(\"SELECT id FROM a\")\n}\n",
            "field `id` on view `V` writes `@id()`",
        ),
    ] {
        refused(source, needle);
    }
}

#[test]
fn refuses_stray_punctuation_after_a_no_argument_attribute() {
    refused(
        &model_with("@readonly,"),
        "writes `@readonly,`: `,` after `@readonly` is not part of any attribute, and a \
         generator reads `@readonly` only when nothing follows it",
    );
    // Every reader matches `@id` exactly since cratestack#1074, so `@id;`
    // is no key anywhere; before it, the model generators took it for one.
    refused(
        "model User {\n  id Int @id;\n}\n",
        "writes `@id;`: `;` after `@id` is not part of any attribute",
    );
}

#[test]
fn a_longer_name_is_another_attribute_and_not_this_rule() {
    // `@unique_per_tenant` is not `@unique` plus text, so it is refused as an
    // unsupported name (it used to stay an unknown, inert attribute), not as
    // stray text after `@unique`.
    let error = parse_schema(&model_with("@unique_per_tenant")).expect_err("unknown name");
    let message = error.to_string();
    assert!(
        message.contains("unsupported attribute `@unique_per_tenant` on a model field"),
        "{message}"
    );
    assert!(!message.contains("after `@unique`"), "{message}");
}

#[test]
fn refuses_attributes_run_together() {
    for (attributes, fix) in [
        ("@readonly@unique", "`@readonly @unique`"),
        ("@unique@pii@sensitive", "`@unique @pii @sensitive`"),
        ("@default(\"x\")@unique", "`@default(\"x\") @unique`"),
    ] {
        refused(
            &model_with(attributes),
            &format!(
                "writes `{attributes}`: attributes with no space between them are read as one \
                 unrecognised attribute, so none of them has any effect. Separate them with a \
                 space: {fix}"
            ),
        );
    }
    refused(
        "model User {\n  id Int @id@unique\n}\n",
        "writes `@id@unique`",
    );
}

// The diagnostic underlines the attribute, as the `@server_only` ones do.
#[test]
fn diagnostics_point_at_the_attribute() {
    for (attributes, written) in [
        ("@readonly()", "@readonly()"),
        ("@pii@sensitive", "@pii@sensitive"),
        ("@unique,", "@unique,"),
    ] {
        let source = model_with(&format!("@unique {attributes}"));
        let error = parse_schema(&source).expect_err("schema should be refused");
        assert_eq!(&source[error.span()], written, "{error}");
    }
}

// Positive controls: `@` inside a string argument, arguments on attributes
// that take them, and every no-argument attribute spelled exactly.
#[test]
fn accepts_at_signs_in_strings_and_correct_spellings() {
    parse_schema(
        "model User {\n  id Int @id\n  email String @unique @email @default(\"x@y\")\n\
         smile String @default(\":) hi@there\")\n\
         pattern String @regex(\"^[a-z]+@[a-z]+$\") @length(min: 3, max: 64)\n\
         note String @readonly @pii @sensitive\n  secret String @server_only\n\
         score Int @range(min: 0, max: 9) @db_enforce\n  rev Int @version\n\
         currency String @iso4217\n  site String @uri\n\
         @@allow(\"read\", auth().email == \"a@b\")\n\
         @@allow('update', auth().email == 'ops@b.io')\n}\n\
         auth Ctx {\n  id Int\n  email String\n}\n\
         type R {\n  s String @length(min: 1)\n}\n\
         procedure p(args: R): R\n",
    )
    .expect("correctly spelled attributes stay accepted");
}

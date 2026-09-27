//! Field attributes written in a form no generator reads
//! (`validate::attribute_spelling`): an argument list or stray punctuation
//! on an attribute that takes no arguments, and two attributes with no
//! space between them.

use super::parse_schema;

#[track_caller]
fn refused(source: &str, needle: &str) {
    let error = parse_schema(source).expect_err("schema should be refused");
    assert!(error.to_string().contains(needle), "error: {error}");
}

fn model_with(attributes: &str) -> String {
    format!("model User {{\n  id Int @id\n  value String {attributes}\n}}\n")
}

// Every entry of the no-argument table, with an argument list, on a model
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
            &format!("writes `{name}()`, but `{name}` takes no arguments — write `{name}`"),
        );
    }
    refused(
        "model User {\n  id Int @id\n  rev Int @version(1)\n}\n",
        "writes `@version(1)`, but `@version` takes no arguments — write `@version`",
    );
    refused(
        "model User {\n  id Int @id(map: \"pk\")\n}\n",
        "writes `@id(map: \"pk\")`, but `@id` takes no arguments — write `@id`",
    );
    for name in ["@email", "@uri", "@iso4217"] {
        refused(&model_with(&format!("{name}()")), "does not take arguments");
        // `validators` refuses the argument list on a model field too, so a
        // `type` field is what shows this rule covers each of the three.
        refused(
            &format!("type R {{\n  s String {name}()\n}}\nprocedure p(): R\n"),
            &format!("writes `{name}()`, but `{name}` takes no arguments — write `{name}`"),
        );
    }
}

// The same rule on the other blocks, which `validators` never sees.
#[test]
fn refuses_an_argument_list_in_every_field_bearing_block() {
    for (source, needle) in [
        (
            "type R {\n  s String @email()\n}\nprocedure p(): R\n",
            "field `s` on type `R` writes `@email()`",
        ),
        (
            "mixin M {\n  s String @readonly()\n}\nmodel A {\n  id Int @id\n}\n",
            "field `s` on mixin `M` writes `@readonly()`",
        ),
        (
            "auth Ctx {\n  id Int\n  s String @pii()\n}\nmodel A {\n  id Int @id\n}\n",
            "field `s` on auth block `Ctx` writes `@pii()`",
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
        "writes `@readonly,`; `@readonly` is recognised only when written exactly `@readonly`",
    );
    // Every reader matches `@id` exactly since cratestack#1074, so `@id;`
    // is no key anywhere; before it, the model generators took it for one.
    refused(
        "model User {\n  id Int @id;\n}\n",
        "writes `@id;`; `@id` is recognised only when written exactly `@id`",
    );
}

#[test]
fn a_longer_name_is_another_attribute_and_not_this_rule() {
    // `@unique_per_tenant` is not `@unique` plus text: it stays an unknown,
    // inert attribute, as it was.
    parse_schema(&model_with("@unique_per_tenant"))
        .expect("an unknown attribute that is not a near-miss stays accepted");
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
                 unrecognised attribute, so this is refused. Separate them with a space: {fix}"
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
         type R {\n  s String @email\n}\n\
         procedure p(): R\n",
    )
    .expect("correctly spelled attributes stay accepted");
}

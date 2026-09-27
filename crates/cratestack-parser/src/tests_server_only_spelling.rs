//! Spellings of `@server_only` that parse but that no generator recognises
//! (`validate::server_only_placement::validate_spelling`), and the span every
//! `@server_only` diagnostic points at.

use super::parse_schema;

#[track_caller]
fn refused(source: &str, needle: &str) {
    let error = parse_schema(source).expect_err("schema should be refused");
    assert!(error.to_string().contains(needle), "error: {error}");
}

// Generators match the raw text `@server_only` exactly, so each of these
// kept a plain model column in every generated output.
#[test]
fn refuses_server_only_with_an_empty_argument_list_on_a_model_scalar() {
    refused(
        "model User {\n  id Int @id\n  passwordHash String @server_only()\n}\n",
        "field `passwordHash` on model `User` writes `@server_only()`",
    );
}

#[test]
fn refuses_server_only_with_an_argument() {
    refused(
        "model User {\n  id Int @id\n  passwordHash String @server_only(true)\n}\n",
        "writes `@server_only(true)`",
    );
}

#[test]
fn refuses_server_only_run_into_the_next_attribute() {
    refused(
        "model User {\n  id Int @id\n  email String @server_only@unique\n}\n",
        "writes `@server_only@unique`",
    );
}

// The same spelling cannot slip one of the four placement rules either.
#[test]
fn refuses_a_misspelled_server_only_on_a_relation_key() {
    refused(
        "model User {\n  id Int @id\n}\n\
         model Post {\n  id Int @id\n  authorId Int @server_only()\n\
         author User @relation(fields: [authorId], references: [id])\n}\n",
        "field `authorId` on model `Post` writes `@server_only()`",
    );
}

#[test]
fn refuses_a_misspelled_server_only_in_every_field_bearing_block() {
    for (source, needle) in [
        (
            "type R {\n  s String @server_only()\n}\nprocedure p(): R\n",
            "on type `R`",
        ),
        (
            // Unused, so only the mixin's own validation sees the field.
            "mixin M {\n  s String @server_only()\n}\nmodel A {\n  id Int @id\n}\n",
            "on mixin `M`",
        ),
        (
            // Used: the field is checked again once expanded into `A`.
            "mixin M {\n  s String @server_only()\n}\nmodel A {\n  @use(M)\n  id Int @id\n}\n",
            "writes `@server_only()`",
        ),
        (
            "auth Ctx {\n  id Int\n  s String @server_only()\n}\nmodel A {\n  id Int @id\n}\n",
            "on auth block `Ctx`",
        ),
        (
            "model A {\n  id Int @id\n  s String\n}\n\
             view V from A {\n  id Int @id @from(A.id)\n  s String @server_only() @from(A.s)\n\
             @@sql(\"SELECT id, s FROM a\")\n}\n",
            "on view `V`",
        ),
    ] {
        refused(source, needle);
    }
}

// Every `@server_only` diagnostic underlines the attribute to remove, not
// the whole field line — that is the range the LSP shows.
#[test]
fn server_only_diagnostics_point_at_the_attribute() {
    for source in [
        "type R {\n  s String @server_only\n}\nprocedure p(): R\n",
        "model User {\n  id Int @id\n  rev Int @version @server_only\n}\n",
        "model User {\n  id Int @id\n}\nmodel Post {\n  id Int @id\n  authorId Int @server_only\n\
         author User @relation(fields: [authorId], references: [id])\n}\n",
        "model User {\n  id Int @id\n}\nmodel Post {\n  id Int @id\n  authorId Int\n\
         author User @relation(fields: [authorId], references: [id]) @server_only\n}\n",
        "model User {\n  id Int @id\n  s String @server_only(x)\n}\n",
    ] {
        let error = parse_schema(source).expect_err("schema should be refused");
        let underlined = &source[error.span()];
        assert!(
            underlined.starts_with("@server_only"),
            "span covers `{underlined}`: {error}"
        );
        assert!(!underlined.contains(' '), "span covers `{underlined}`");
    }
}

// Removing `@server_only` from a foreign key re-opens it to client input
// (the generated create/update inputs filtered it out). The diagnostic says
// how to keep that half, and the suggested replacement parses.
#[test]
fn relation_key_diagnostic_offers_readonly_and_readonly_is_accepted() {
    refused(
        "model User {\n  id Int @id\n}\n\
         model Post {\n  id Int @id\n  authorId Int @server_only\n\
         author User @relation(fields: [authorId], references: [id])\n}\n",
        "to stop clients from setting the key, mark it @readonly instead",
    );
    parse_schema(
        "model User {\n  id Int @id\n}\n\
         model Post {\n  id Int @id\n  authorId Int @readonly\n\
         author User @relation(fields: [authorId], references: [id])\n}\n",
    )
    .expect("@readonly on a foreign key stays accepted");
}

//! M17: the single-source entry points refuse a directive
//! (docs/design/multi-file-schemas.md section 9).

use crate::{parse_schema, parse_schema_diagnostics, parse_schema_named, parse_schema_unvalidated};

const SOURCE: &str = "model A {\n  id Int @id\n}\n\n  part \"models/post.cstack\"\n";

fn m17(directive: &str, path: &str) -> String {
    format!(
        "`{directive}` needs the schema to be loaded from a file, because \"{path}\" is resolved \
         against that file's directory; use `parse_schema_file` or `cratestack check --schema \
         <root>`"
    )
}

fn check(error: &crate::SchemaError, directive: &str, path: &str, source: &str) {
    assert_eq!(error.message(), m17(directive, path));
    let span = error.span();
    assert_eq!(&source[span], format!("{directive} \"{path}\""));
    assert!(error.related().is_empty());
}

#[test]
fn every_single_source_entry_point_refuses_a_directive() {
    let error = parse_schema(SOURCE).unwrap_err();
    check(&error, "part", "models/post.cstack", SOURCE);
    assert_eq!(error.line(), 5);
    assert_eq!(error.file(), crate::ANONYMOUS_SCHEMA);

    let error = parse_schema_named("schema.cstack", SOURCE).unwrap_err();
    check(&error, "part", "models/post.cstack", SOURCE);
    assert_eq!(error.file(), "schema.cstack");

    let (schema, errors) = parse_schema_diagnostics("schema.cstack", SOURCE);
    assert!(schema.is_none());
    assert_eq!(errors.len(), 1);
    check(&errors[0], "part", "models/post.cstack", SOURCE);

    let error = parse_schema_unvalidated(SOURCE).unwrap_err();
    check(&error, "part", "models/post.cstack", SOURCE);
}

#[test]
fn the_first_directive_in_source_order_is_the_one_reported() {
    let source = "import \"a.cstack\"\npart of \"b.cstack\"\npart \"c.cstack\"\n";
    let error = parse_schema(source).unwrap_err();
    check(&error, "import", "a.cstack", source);

    let source = "part \"c.cstack\"\nimport \"a.cstack\"\n";
    check(
        &parse_schema(source).unwrap_err(),
        "part",
        "c.cstack",
        source,
    );

    let source = "model A {\n  id Int @id\n}\npart of \"../s.cstack\"\n";
    check(
        &parse_schema(source).unwrap_err(),
        "part of",
        "../s.cstack",
        source,
    );
}

#[test]
fn a_directive_stops_before_any_later_validation_error() {
    let source = "part \"a.cstack\"\nmodel A {\n  id Int @id\n  other Missing\n}\n";
    let error = parse_schema(source).unwrap_err();
    check(&error, "part", "a.cstack", source);
    let (_, errors) = parse_schema_diagnostics("s.cstack", source);
    assert_eq!(errors.len(), 1);
}

#[test]
fn a_directive_free_source_is_unaffected() {
    let source = "model A {\n  id Int @id\n  @@allow(\"read\", true)\n}\n";
    assert_eq!(parse_schema(source).unwrap().models.len(), 1);
}

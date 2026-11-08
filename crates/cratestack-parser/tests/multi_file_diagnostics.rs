//! Errors from several files in one run, attributed to the right file with
//! local spans (cratestack#911).

mod multi_file_support;

use multi_file_support::{HEAD, ROOT, assert_primary, load, load_all_errors};

const A: &str = "/fixture/a.cstack";
const B: &str = "/fixture/b.cstack";

#[test]
fn validation_errors_from_two_parts_arrive_in_one_run() {
    let root = format!("{HEAD}part \"a.cstack\"\npart \"b.cstack\"\n");
    let a = "part of \"schema.cstack\"\n\nmodel A {\n  id Int @id\n  owner Missing\n}\n";
    let b = "part of \"schema.cstack\"\n\n\ntype B {\n  other Absent\n}\n";
    let files = [(ROOT, root.as_str()), (A, a), (B, b)];
    let errors = load_all_errors(&files);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_primary(
        &files,
        &errors[0],
        "unknown type `Missing`",
        A,
        "  owner Missing",
    );
    assert_eq!(errors[0].line(), 5);
    assert_primary(
        &files,
        &errors[1],
        "unknown type `Absent`",
        B,
        "  other Absent",
    );
    assert_eq!(errors[1].line(), 5);
    assert_ne!(errors[0].file(), errors[1].file());
    assert!(errors[0].render().contains("a.cstack"));
}

#[test]
fn load_schema_is_the_first_of_the_diagnostics() {
    let root = format!("{HEAD}part \"a.cstack\"\npart \"b.cstack\"\n");
    let a = "part of \"schema.cstack\"\n\nmodel A {\n  id Int @id\n  owner Missing\n}\n";
    let b = "part of \"schema.cstack\"\n\ntype B {\n  other Absent\n}\n";
    let files = [(ROOT, root.as_str()), (A, a), (B, b)];
    let first = load(&files).unwrap_err();
    let all = load_all_errors(&files);
    assert_eq!(first.message(), all[0].message());
    assert_eq!((first.file(), first.span()), (all[0].file(), all[0].span()));
}

#[test]
fn a_syntax_error_in_a_part_is_the_single_error_and_is_attributed_to_it() {
    let root = format!("{HEAD}part \"a.cstack\"\npart \"b.cstack\"\n");
    let a = "part of \"schema.cstack\"\n\nmodel A {\n  id Int @id\n  owner Missing\n}\n";
    let b = "part of \"schema.cstack\"\n\n\nwidget Nope\n";
    let files = [(ROOT, root.as_str()), (A, a), (B, b)];
    let errors = load_all_errors(&files);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_primary(
        &files,
        &errors[0],
        "unsupported top-level declaration: widget Nope",
        B,
        "widget Nope",
    );
    assert_eq!(errors[0].line(), 4);
}

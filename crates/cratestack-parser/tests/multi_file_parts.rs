//! `part` / `part of` loading and every part rule, M1 to M9 and M14, M15
//! (docs/design/multi-file-schemas.md sections 7 and 9). All on fake paths.

mod multi_file_support;

use std::io::ErrorKind;

use cratestack_parser::{FileRole, LoadOptions, load_schema_diagnostics};
use multi_file_support::{HEAD, ROOT, assert_primary, assert_related, load, load_from, overlay};

const USER: &str = "/fixture/models/user.cstack";
const POST: &str = "/fixture/models/post.cstack";

const ROOT_TEXT: &str = "datasource db {\n  provider = \"postgresql\"\n}\n\npart \
    \"models/user.cstack\"\npart \"models/post.cstack\"\n\nmixin Stamps {\n  createdAt \
    DateTime\n}\n\nmodel Root {\n  id Int @id\n}\n";
const USER_TEXT: &str = "part of \"../schema.cstack\"\n\nmodel User {\n  @use(Stamps)\n  id Int \
    @id\n}\n";
const POST_TEXT: &str = "part of \"../schema.cstack\"\n\nmodel Post {\n  id Int @id\n  authorId \
    Int\n  author User @relation(fields:[authorId],references:[id])\n}\n";

fn three() -> Vec<(&'static str, &'static str)> {
    vec![(ROOT, ROOT_TEXT), (USER, USER_TEXT), (POST, POST_TEXT)]
}

fn names(loaded: &cratestack_parser::LoadedSchema) -> Vec<&str> {
    loaded
        .schema
        .models
        .iter()
        .map(|m| m.name.as_str())
        .collect()
}

#[test]
fn a_root_and_two_parts_merge_in_listed_order() {
    let loaded = load(&three()).expect("loads");
    assert_eq!(names(&loaded), ["Root", "User", "Post"]);
}

#[test]
fn a_part_can_use_a_mixin_and_a_relation_target_from_another_file() {
    let loaded = load(&three()).expect("loads");
    let user = &loaded.schema.models[1];
    let fields: Vec<_> = user.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        fields,
        ["createdAt", "id"],
        "mixin from the root, used in a part"
    );
    assert!(
        loaded.schema.models[2]
            .fields
            .iter()
            .any(|f| f.name == "author")
    );
}

#[test]
fn extensions_union_across_parts() {
    let files = [
        (
            ROOT,
            "part \"a.cstack\"\npart \"b.cstack\"\nextension pgvector {\n}\n",
        ),
        (
            "/fixture/a.cstack",
            "part of \"schema.cstack\"\nextension rate_limit {\n}\n",
        ),
        (
            "/fixture/b.cstack",
            "part of \"schema.cstack\"\nextension pgvector {\n}\n",
        ),
    ];
    let loaded = load(&files).expect("loads");
    let mut kinds: Vec<_> = loaded
        .schema
        .declared_extensions
        .iter()
        .map(|k| k.as_str())
        .collect();
    kinds.sort_unstable();
    assert_eq!(kinds, ["pgvector", "rate_limit"]);
}

#[test]
fn load_reports_files_in_load_order_with_roles_and_bases() {
    let loaded = load(&three()).expect("loads");
    let (a, b, c) = (ROOT_TEXT.len(), USER_TEXT.len(), POST_TEXT.len());
    let seen: Vec<_> = loaded
        .files
        .iter()
        .map(|f| (f.display.as_str(), f.relative.as_str(), f.base, f.role))
        .collect();
    assert_eq!(
        seen,
        [
            (ROOT, "schema.cstack", 0, FileRole::Root),
            (USER, "models/user.cstack", a + 1, FileRole::Part),
            (POST, "models/post.cstack", a + 1 + b + 1, FileRole::Part),
        ]
    );
    assert_eq!(loaded.root().display, ROOT);
    assert!(!loaded.is_single_file());
    let _ = c;
}

#[test]
fn locate_maps_global_offsets_back_to_the_right_file() {
    let loaded = load(&three()).expect("loads");
    let (a, b, c) = (ROOT_TEXT.len(), USER_TEXT.len(), POST_TEXT.len());
    let at = |offset| {
        loaded
            .locate(offset)
            .map(|(f, local)| (f.relative.clone(), local))
    };
    assert_eq!(at(0), Some(("schema.cstack".into(), 0)));
    assert_eq!(
        at(a),
        Some(("schema.cstack".into(), a)),
        "end of file belongs to its file"
    );
    assert_eq!(at(a + 1), Some(("models/user.cstack".into(), 0)));
    assert_eq!(at(a + 1 + b), Some(("models/user.cstack".into(), b)));
    assert_eq!(at(a + b + 2 + 5), Some(("models/post.cstack".into(), 5)));
    assert_eq!(at(a + b + c + 2), Some(("models/post.cstack".into(), c)));
    assert_eq!(at(a + b + c + 3), None);
}

#[test]
fn a_part_without_part_of_is_m1() {
    let files = [
        (ROOT, "part \"models/user.cstack\"\n"),
        (USER, "model U {\n  id Int @id\n}\n"),
    ];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`models/user.cstack` is listed as a part of `schema.cstack`, but does not declare \
         `part of`; add `part of \"../schema.cstack\"` to `models/user.cstack`",
        ROOT,
        "part \"models/user.cstack\"",
    );
    assert!(error.related().is_empty());
    assert_eq!(error.line(), 1);
}

#[test]
fn a_part_naming_another_root_is_m2() {
    let files = [
        (ROOT, "part \"models/user.cstack\"\n"),
        (USER, "\npart of \"../other.cstack\"\n"),
    ];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`models/user.cstack` is listed as a part of `schema.cstack`, but declares itself a part \
         of `other.cstack`",
        USER,
        "part of \"../other.cstack\"",
    );
    assert_eq!(error.line(), 2);
    assert_related(
        &files,
        &error,
        ROOT,
        "part \"models/user.cstack\"",
        "`models/user.cstack` is listed here",
    );
}

#[test]
fn check_style_load_of_an_unlisted_part_is_m3() {
    let files = [(ROOT, "model R {\n  id Int @id\n}\n"), (USER, USER_TEXT)];
    let error = load_from(&files, USER, true).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`models/user.cstack` declares itself a part of `schema.cstack`, but `schema.cstack` \
         does not list it; add `part \"models/user.cstack\"` to `schema.cstack`",
        USER,
        "part of \"../schema.cstack\"",
    );
}

#[test]
fn a_part_given_as_root_is_m4() {
    let files = [(ROOT, ROOT_TEXT), (USER, USER_TEXT)];
    let error = load_from(&files, USER, false).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`/fixture/models/user.cstack` is a part of `/fixture/schema.cstack`, not a schema root; \
         point the macro or command at `/fixture/schema.cstack`",
        USER,
        "part of \"../schema.cstack\"",
    );
}

#[test]
fn following_part_of_from_a_part_loads_the_whole_schema() {
    let loaded = load_from(&three(), POST, true).expect("loads from the root it names");
    assert_eq!(loaded.root().display, ROOT);
    assert_eq!(loaded.files[0].role, FileRole::Root);
    assert_eq!(names(&loaded), ["Root", "User", "Post"]);
}

fn root_only(part_body: &str) -> [(&'static str, String); 2] {
    [
        (ROOT, "part \"models/user.cstack\"\n".to_owned()),
        (
            USER,
            format!("part of \"../schema.cstack\"\n\n{part_body}\n"),
        ),
    ]
}

#[test]
fn root_only_blocks_in_a_part_are_m5() {
    let cases = [
        (
            "datasource",
            "datasource db {\n  provider = \"postgresql\"\n}",
        ),
        ("auth", "auth Principal {\n  id Int\n}"),
        ("transport", "transport rpc"),
        ("mcp", "mcp {\n  name = \"blog\"\n  expose = [tools]\n}"),
    ];
    for (kind, block) in cases {
        let owned = root_only(block);
        let files: Vec<(&str, &str)> = owned.iter().map(|(p, t)| (*p, t.as_str())).collect();
        let error = load(&files).unwrap_err();
        assert_primary(
            &files,
            &error,
            &format!("`{kind}` belongs in the root schema (`schema.cstack`), not in a part"),
            USER,
            block,
        );
        assert_eq!(error.line(), 3);
    }
}

#[test]
fn the_same_four_in_a_root_are_fine() {
    let root = format!(
        "{HEAD}auth Principal {{\n  id Int\n}}\n\ntransport rpc\n\nmcp {{\n  \
         expose = [tools]\n}}\n\nprocedure ping(args: Principal): Principal\n  @allow(true)\n  \
         @mcp(tool)\n\npart \"models/user.cstack\"\n"
    );
    let files = [
        (ROOT, root.as_str()),
        (
            USER,
            "part of \"../schema.cstack\"\n\nmodel U {\n  id Int @id\n}\n",
        ),
    ];
    let loaded = load(&files).expect("a root may declare all four");
    assert!(loaded.schema.datasource.is_some() && loaded.schema.auth.is_some());
    assert!(loaded.schema.mcp.is_some());
}

#[test]
fn a_part_listing_a_part_is_m6() {
    let files = [
        (ROOT, "part \"models/user.cstack\"\n"),
        (USER, "part of \"../schema.cstack\"\npart \"x.cstack\"\n"),
    ];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "a part cannot list parts of its own; move `part \"x.cstack\"` to the root schema \
         (`schema.cstack`)",
        USER,
        "part \"x.cstack\"",
    );
}

#[test]
fn listing_one_file_twice_is_m8() {
    let files = [
        (ROOT, "part \"a.cstack\"\npart \"./x/../a.cstack\"\n"),
        ("/fixture/a.cstack", "part of \"schema.cstack\"\n"),
    ];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`./x/../a.cstack` is already listed as a part on line 1",
        ROOT,
        "part \"./x/../a.cstack\"",
    );
    assert_eq!(error.line(), 2);
}

#[test]
fn a_root_listing_itself_is_m9() {
    let files = [(ROOT, "\npart \"schema.cstack\"\n")];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`schema.cstack` lists itself as a part",
        ROOT,
        "part \"schema.cstack\"",
    );
    assert_eq!(error.line(), 2);
}

#[test]
fn a_missing_part_is_m14() {
    let files = [(
        ROOT,
        "model R {\n  id Int @id\n}\npart \"missing.cstack\"\n",
    )];
    let error = load(&files).unwrap_err();
    let io = std::io::Error::from(ErrorKind::NotFound);
    assert_primary(
        &files,
        &error,
        &format!("cannot read `missing.cstack` (listed on line 4 of `schema.cstack`): {io}"),
        ROOT,
        "part \"missing.cstack\"",
    );
}

#[test]
fn bad_paths_are_m15() {
    let cases = [
        ("part", "part \"/abs/x.cstack\"\n", "/abs/x.cstack"),
        (
            "part",
            "part \"models\\\\x.cstack\"\n",
            "models\\\\x.cstack",
        ),
        ("part", "part \"models/x\"\n", "models/x"),
        ("part", "part \"\"\n", ""),
    ];
    for (directive, root, path) in cases {
        let files = [(ROOT, root)];
        let error = load(&files).unwrap_err();
        assert_primary(
            &files,
            &error,
            &format!(
                "`{directive}` path `{path}` must be a relative path written with `/` and \
                 ending in `.cstack`"
            ),
            ROOT,
            path,
        );
    }
    let files = [
        (ROOT, "part \"models/user.cstack\"\n"),
        (USER, "part of \"../schema\"\n"),
    ];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`part of` path `../schema` must be a relative path written with `/` and ending in \
         `.cstack`",
        USER,
        "../schema",
    );
}

#[test]
fn the_files_a_failed_load_tried_to_read_are_reported() {
    let sources = overlay(&[(ROOT, "part \"missing.cstack\"\n")]);
    let options = LoadOptions {
        follow_part_of: false,
        sources: Some(&sources),
    };
    let diagnostics = load_schema_diagnostics(ROOT, &options);
    assert!(diagnostics.schema.is_none());
    let files: Vec<_> = diagnostics
        .files
        .iter()
        .map(|p| p.to_str().unwrap())
        .collect();
    assert_eq!(files, [ROOT, "/fixture/missing.cstack"]);
}

#[test]
fn an_unreadable_root_keeps_todays_message() {
    let error = load(&[]).unwrap_err();
    let io = std::io::Error::from(ErrorKind::NotFound);
    assert_eq!(
        error.message(),
        format!("failed to read schema file {ROOT}: {io}")
    );
    assert_eq!((error.file(), error.span(), error.line()), (ROOT, 0..0, 1));
}

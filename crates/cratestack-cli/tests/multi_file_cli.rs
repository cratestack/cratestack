//! cratestack#914: every `--schema` command loads the whole graph from the
//! root; only `check` accepts a part (docs/design/multi-file-schemas.md
//! section 11). These run the real binary and assert stdout, stderr and exit
//! codes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cratestack-parser/tests/fixtures/multi_file")
        .canonicalize()
        .unwrap()
}

fn split_root() -> String {
    fixtures().join("split/schema.cstack").display().to_string()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cratestack"))
        .env("RUST_BACKTRACE", "0")
        .args(args)
        .output()
        .expect("the cratestack binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// stderr without ANSI colour codes.
fn stderr(out: &Output) -> String {
    let raw = String::from_utf8(out.stderr.clone()).unwrap();
    let mut clean = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            clean.push(c);
        }
    }
    clean
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).expect("stdout is JSON")
}

/// A temp dir holding `files` (relative name, text).
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn at(dir: &tempfile::TempDir, name: &str) -> String {
    dir.path().join(name).display().to_string()
}

fn files_under(dir: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(dir).unwrap().display().to_string();
                found.push((rel, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn check_on_a_root_lists_files_with_roles() {
    let root = split_root();
    let human = run(&["check", "--schema", &root, "--list-files"]);
    assert_eq!(human.status.code(), Some(0));
    let dir = fixtures().join("split").display().to_string();
    let expected = format!(
        "schema OK: {root}\nfiles (7):\n  root        {root}\n  part        {dir}/models/user.cstack\n  \
         part        {dir}/models/post.cstack\n  part        {dir}/api.cstack\n  \
         import      {dir}/shared/money.cstack\n  import part {dir}/shared/money/currency.cstack\n  \
         import      {dir}/shared/ids.cstack\n"
    );
    assert_eq!(stdout(&human), expected);

    let out = run(&[
        "check",
        "--schema",
        &root,
        "--list-files",
        "--format",
        "json",
    ]);
    let value = json(&out);
    let roles: Vec<_> = value["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["role"].as_str().unwrap())
        .collect();
    assert_eq!(
        roles,
        [
            "root",
            "part",
            "part",
            "part",
            "import",
            "import part",
            "import"
        ]
    );
    assert_eq!(value["files"][0]["path"], root.as_str());
    assert!(value.get("root").is_none(), "no redirect, no root key");
}

#[test]
fn check_on_a_part_checks_the_whole_schema_and_says_so() {
    let root = split_root();
    let part = fixtures()
        .join("split/models/post.cstack")
        .display()
        .to_string();
    let out = run(&["check", "--schema", &part]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        format!("`{part}` is a part of `{root}`; checked the whole schema\nschema OK: {root}\n")
    );
    let value = json(&run(&["check", "--schema", &part, "--format", "json"]));
    assert_eq!(value["schema"], part.as_str());
    assert_eq!(value["root"], root.as_str());
}

#[test]
fn check_on_an_unlisted_part_is_m3_and_exits_nonzero() {
    let dir = tree(&[
        ("schema.cstack", "type A {\n  x Int\n}\n"),
        (
            "orphan.cstack",
            "part of \"schema.cstack\"\n\ntype B {\n  y Int\n}\n",
        ),
    ]);
    let out = run(&["check", "--schema", &at(&dir, "orphan.cstack")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("does not list it"),
        "{}",
        stderr(&out)
    );
    let out = run(&[
        "check",
        "--schema",
        &at(&dir, "orphan.cstack"),
        "--format",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["ok"], false);
}

const DUP_ROOT: &str = "part \"a.cstack\"\npart \"b.cstack\"\n";

#[test]
fn check_json_failure_carries_related_for_a_cross_file_duplicate() {
    let dir = tree(&[
        ("schema.cstack", DUP_ROOT),
        (
            "a.cstack",
            "part of \"schema.cstack\"\ntype Dup {\n  x Int\n}\n",
        ),
        (
            "b.cstack",
            "part of \"schema.cstack\"\ntype Dup {\n  y Int\n}\n",
        ),
    ]);
    let schema = at(&dir, "schema.cstack");
    let out = run(&["check", "--schema", &schema, "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let diagnostic = &json(&out)["diagnostics"][0];
    assert_eq!(diagnostic["file"], at(&dir, "b.cstack").as_str());
    let related = &diagnostic["related"][0];
    assert_eq!(related["file"], at(&dir, "a.cstack").as_str());
    assert_eq!(related["line"], 2);
    for key in ["start", "end", "message"] {
        assert!(related.get(key).is_some(), "related has {key}");
    }

    // Human mode renders both code frames.
    let human = stderr(&run(&["check", "--schema", &schema]));
    assert!(
        human.contains("a.cstack:2") && human.contains("b.cstack:2"),
        "{human}"
    );
}

#[test]
fn check_json_failure_without_related_is_unchanged() {
    let dir = tree(&[("s.cstack", "model User {\n  email String\n}\n")]);
    let schema = at(&dir, "s.cstack");
    let out = run(&["check", "--schema", &schema, "--format", "json"]);
    let value = json(&out);
    let mut keys: Vec<_> = value["diagnostics"][0]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, ["end", "file", "line", "message", "start"]);
    let mut top: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    top.sort();
    assert_eq!(top, ["diagnostics", "ok", "schema"]);
}

#[test]
fn check_on_a_single_file_is_byte_identical_to_before() {
    let dir = tree(&[("s.cstack", "model Item {\n  id Int @id\n}\n")]);
    let schema = at(&dir, "s.cstack");
    assert_eq!(
        stdout(&run(&["check", "--schema", &schema])),
        format!("schema OK: {schema}\n")
    );
    assert_eq!(
        stdout(&run(&["check", "--schema", &schema, "--format", "json"])),
        format!("{{\n  \"diagnostics\": [],\n  \"ok\": true,\n  \"schema\": {schema:?}\n}}\n")
    );
    let listed = stdout(&run(&["check", "--schema", &schema, "--list-files"]));
    assert_eq!(
        listed,
        format!("schema OK: {schema}\nfiles (1):\n  root        {schema}\n")
    );
}

#[test]
fn print_ir_no_spans_is_identical_for_split_and_concatenated() {
    let concatenated = fixtures().join("concatenated.cstack").display().to_string();
    let a = run(&["print-ir", "--schema", &split_root(), "--no-spans"]);
    let b = run(&["print-ir", "--schema", &concatenated, "--no-spans"]);
    assert_eq!(a.status.code(), Some(0));
    assert_eq!(stdout(&a), stdout(&b));
    let with_spans = run(&["print-ir", "--schema", &split_root()]);
    assert_ne!(
        stdout(&with_spans),
        stdout(&a),
        "spans are still printed by default"
    );
}

fn generate_both(kind: &str, extra: &[&str]) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let concatenated = fixtures().join("concatenated.cstack").display().to_string();
    let mut trees = Vec::new();
    for schema in [split_root(), concatenated] {
        let out = tempfile::tempdir().unwrap();
        let out_arg = out.path().display().to_string();
        let mut args = vec![kind, "--schema", &schema, "--out", &out_arg];
        args.extend_from_slice(extra);
        let result = run(&args);
        assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
        trees.push(files_under(out.path()));
    }
    let b = trees.pop().unwrap();
    (trees.pop().unwrap(), b)
}

#[test]
fn generate_typescript_from_a_split_schema_equals_the_concatenated_one() {
    let (split, whole) = generate_both("generate-typescript", &[]);
    assert!(!split.is_empty());
    assert_eq!(split, whole);
}

#[test]
fn generate_dart_from_a_split_schema_equals_the_concatenated_one() {
    let (split, whole) = generate_both("generate-dart", &[]);
    assert!(!split.is_empty());
    assert_eq!(split, whole);
}

#[test]
fn migrate_diff_after_a_split_writes_no_migration() {
    let concatenated = fixtures().join("concatenated.cstack").display().to_string();
    let out = tempfile::tempdir().unwrap();
    let out_dir = out.path().display().to_string();
    let base = [
        "migrate",
        "diff",
        "--out-dir",
        &out_dir,
        "--backend",
        "postgres",
    ];
    let first = run(&[&base[..], &["--schema", &concatenated]].concat());
    assert_eq!(first.status.code(), Some(0), "{}", stderr(&first));
    let before = files_under(out.path());
    assert!(!before.is_empty());

    let second = run(&[&base[..], &["--schema", &split_root()]].concat());
    assert_eq!(second.status.code(), Some(0), "{}", stderr(&second));
    assert!(
        stdout(&second).contains("no changes"),
        "{}",
        stdout(&second)
    );
    assert_eq!(files_under(out.path()), before);
}

#[test]
fn a_part_given_to_generate_typescript_is_m4() {
    let part = fixtures()
        .join("split/models/post.cstack")
        .display()
        .to_string();
    let out = tempfile::tempdir().unwrap();
    let result = run(&[
        "generate-typescript",
        "--schema",
        &part,
        "--out",
        &out.path().display().to_string(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    let root = fixtures().join("split/schema.cstack").display().to_string();
    assert!(
        stderr(&result).contains(&format!(
            "`{part}` is a part of `{root}`, not a schema root; point the macro or command at \
             `{root}`"
        )),
        "{}",
        stderr(&result)
    );
    assert!(files_under(out.path()).is_empty(), "nothing is generated");
}
